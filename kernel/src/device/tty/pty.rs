use crate::{
    device::Device as CharacterDevice,
    device::tty::{Tty, TtyDriver, TtyFileOps},
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::{Identity, signal::Signal},
    sched::Scheduler,
    uapi::{self, termios::winsize},
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{
        self, File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
        fs::devtmpfs,
        inode::Mode,
    },
};
use alloc::{
    collections::btree_map::BTreeMap,
    format,
    sync::{Arc, Weak},
    vec,
};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};

/// Next PTY index to allocate.
static PTY_INDEX: AtomicU32 = AtomicU32::new(0);

/// Global table of live PTY pairs (index -> pair). Entries are removed when
/// the master is closed.
static PTY_TABLE: SpinMutex<BTreeMap<u32, Arc<PtyPair>>> = SpinMutex::new(BTreeMap::new());

const MASTER_BUF_SIZE: usize = 0x1000;

pub struct PtyPair {
    /// Slave index N (for `/dev/pts/N`).
    pub index: u32,
    /// The TTY object associated with the slave side.
    pub tty: Arc<Tty>,
    /// Slave -> master buffer (program output that the master reads).
    pub master_buf: SpinMutex<RingBuffer>,
    /// Signalled when data is pushed into `master_buf` or when the master
    /// needs to re-check state (e.g. packet mode flags, slave going away).
    pub master_rd_event: Event,
    /// Signalled when space becomes available in `master_buf`.
    pub master_wr_event: Event,
    /// Slave may be opened via `/dev/pts/N`.
    pub unlocked: AtomicBool,
    /// Open slave file descriptor count.
    pub slave_count: AtomicUsize,
    /// TIOCPKT packet mode flag.
    pub packet: AtomicBool,
    /// Pending TIOCPKT control bits (STOP/START/FLUSH).
    /// Consumed on the next master read when packet mode is active.
    pub packet_flags: AtomicU8,
}

/// TtyDriver implementation for the slave side: slave `write()` routes here,
/// which stores bytes in the master's buffer.
struct PtySlaveTtyDriver {
    pair: Weak<PtyPair>,
}

impl TtyDriver for PtySlaveTtyDriver {
    fn write_output(&self, data: &[u8]) -> EResult<()> {
        let Some(pair) = self.pair.upgrade() else {
            return Ok(());
        };
        let mut written = 0;
        while written < data.len() {
            let guard = pair.master_wr_event.guard();
            let count = {
                let mut buf = pair.master_buf.lock();
                buf.write(&data[written..])
            };

            if count != 0 {
                written += count;
                pair.master_rd_event.wake_all();
                continue;
            }

            if pair.tty.is_hung_up() {
                return Err(Errno::EIO);
            }

            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::ERESTART);
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::ERESTART);
            }
        }
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        winsize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

fn has_open_slave(pair: &PtyPair) -> bool {
    pair.slave_count.load(Ordering::Acquire) != 0
}

pub fn alloc_pty() -> EResult<Arc<PtyPair>> {
    let index = PTY_INDEX.fetch_add(1, Ordering::Relaxed);
    let name = format!("pts/{}", index);

    let pair = Arc::new_cyclic(|weak: &Weak<PtyPair>| {
        let driver = Arc::new(PtySlaveTtyDriver { pair: weak.clone() });
        let tty = Tty::new(name.clone(), driver);
        PtyPair {
            index,
            tty,
            master_buf: SpinMutex::new(RingBuffer::new(MASTER_BUF_SIZE)),
            master_rd_event: Event::new(),
            master_wr_event: Event::new(),
            unlocked: AtomicBool::new(false),
            slave_count: AtomicUsize::new(0),
            packet: AtomicBool::new(false),
            packet_flags: AtomicU8::new(0),
        }
    });

    PTY_TABLE.lock().insert(index, pair.clone());

    let slave_ops: Arc<dyn CharacterDevice> = Arc::new(PtySlaveFileOps {
        tty_ops: TtyFileOps {
            tty: pair.tty.clone(),
        },
        pair: Arc::downgrade(&pair),
        index,
    });
    pair.tty.clone().register_device_with_opener(slave_ops)?;

    Ok(pair)
}

pub struct PtySlaveFileOps {
    tty_ops: TtyFileOps,
    pair: Weak<PtyPair>,
    index: u32,
}

impl CharacterDevice for PtySlaveFileOps {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        let pair = self.pair.upgrade().ok_or(Errno::EIO)?;
        if !pair.unlocked.load(Ordering::Acquire) {
            return Err(Errno::EIO);
        }
        pair.slave_count.fetch_add(1, Ordering::AcqRel);

        Ok(Arc::new(PtySlaveFile {
            tty_ops: TtyFileOps {
                tty: self.tty_ops.tty.clone(),
            },
            pair,
        }))
    }

    fn major(&self) -> u32 {
        136
    }

    fn minor(&self) -> u32 {
        self.index
    }
}

pub struct PtySlaveFile {
    tty_ops: TtyFileOps,
    pair: Arc<PtyPair>,
}

impl Drop for PtySlaveFile {
    fn drop(&mut self) {
        self.pair.slave_count.fetch_sub(1, Ordering::AcqRel);
        self.pair.master_rd_event.wake_all();
    }
}

impl FileOps for PtySlaveFile {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        self.tty_ops.read(file, buffer, offset)
    }

    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        self.tty_ops.write(file, buffer, offset)
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.tty_ops.ioctl(file, request, arg)
    }

    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        self.tty_ops.poll(file, mask)
    }

    fn poll_events(&self, file: &File, mask: PollFlags) -> PollEventSet<'_> {
        self.tty_ops.poll_events(file, mask)
    }
}

/// FileOps for the master side of a PTY pair.
pub struct PtyMaster {
    pub pair: Arc<PtyPair>,
}

impl PtyMaster {
    fn queue_echo_output(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let wrote_any = {
            let mut buf = self.pair.master_buf.lock();
            buf.write(data) != 0
        };

        if wrote_any {
            self.pair.master_rd_event.wake_all();
        }
    }
}

impl FileOps for PtyMaster {
    fn read(&self, file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        loop {
            let guard = self.pair.master_rd_event.guard();

            // Packet-mode: a pending control byte takes priority over data.
            if self.pair.packet.load(Ordering::Relaxed) {
                let pending = self.pair.packet_flags.swap(0, Ordering::AcqRel);
                if pending != 0 {
                    return buffer.copy_from_slice(&[pending]).map(|_| 1isize);
                }
            }

            {
                let mut mbuf = self.pair.master_buf.lock();
                if !mbuf.is_empty() {
                    let packet = self.pair.packet.load(Ordering::Relaxed);
                    if packet {
                        if buffer.len() < 2 {
                            return buffer
                                .copy_from_slice(&[uapi::ioctls::TIOCPKT_DATA as u8])
                                .map(|_| 1isize);
                        }
                        let cap = buffer.len() - 1;
                        let len = core::cmp::min(mbuf.get_data_len(), cap);
                        let mut tmp = vec![0u8; 1 + len];
                        tmp[0] = uapi::ioctls::TIOCPKT_DATA as u8;
                        let n = mbuf.read(&mut tmp[1..1 + len]);
                        drop(mbuf);
                        self.pair.master_wr_event.wake_all();
                        return buffer
                            .copy_from_slice(&tmp[..1 + n])
                            .map(|_| (1 + n) as isize);
                    } else {
                        let len = core::cmp::min(mbuf.get_data_len(), buffer.len());
                        let mut tmp = vec![0u8; len];
                        let n = mbuf.read(&mut tmp);
                        drop(mbuf);
                        self.pair.master_wr_event.wake_all();
                        return buffer.copy_from_slice(&tmp[..n]).map(|_| n as isize);
                    }
                }
            }

            if !has_open_slave(&self.pair) {
                return Err(Errno::EIO);
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            }

            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::ERESTART);
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::ERESTART);
            }
        }
    }

    fn write(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let total = buffer.len();
        let mut data = vec![0u8; total];
        buffer.copy_to_slice(&mut data)?;

        // Feed each byte through the slave's line discipline.
        for &byte in &data {
            let echo = self.pair.tty.input_byte_internal(byte);
            self.queue_echo_output(&echo);
        }

        Ok(total as isize)
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        match request as u32 {
            uapi::ioctls::TIOCGPTN => {
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(self.pair.index as i32).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            uapi::ioctls::TIOCSPTLCK => {
                let ptr: UserPtr<i32> = UserPtr::new(arg);
                let lock = ptr.read().ok_or(Errno::EFAULT)?;
                self.pair.unlocked.store(lock == 0, Ordering::Release);
                Ok(0)
            }
            uapi::ioctls::TIOCGPTLCK => {
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                let locked = if self.pair.unlocked.load(Ordering::Acquire) {
                    0
                } else {
                    1
                };
                ptr.write(locked).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            uapi::ioctls::TIOCPKT => {
                let ptr: UserPtr<i32> = UserPtr::new(arg);
                let enable = ptr.read().ok_or(Errno::EFAULT)? != 0;
                self.pair.packet.store(enable, Ordering::Release);
                if !enable {
                    self.pair.packet_flags.store(0, Ordering::Release);
                }
                Ok(0)
            }
            uapi::ioctls::TIOCGPKT => {
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                let enabled = self.pair.packet.load(Ordering::Acquire) as i32;
                ptr.write(enabled).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            uapi::ioctls::TIOCSIG => {
                let ptr: UserPtr<i32> = UserPtr::new(arg);
                let sig_num = ptr.read().ok_or(Errno::EFAULT)? as u32;
                let sig = Signal::try_from(sig_num).map_err(|_| Errno::EINVAL)?;
                self.pair.tty.signal_foreground(sig);
                Ok(0)
            }
            _ => {
                // Forward other ioctls (TCGETS, TIOCGWINSZ, etc.) to the slave TTY.
                let ops = TtyFileOps {
                    tty: self.pair.tty.clone(),
                };
                ops.ioctl(file, request, arg)
            }
        }
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let mut revents = PollFlags::empty();
        if mask.contains(PollFlags::In) {
            let has_data = !self.pair.master_buf.lock().is_empty();
            let has_packet = self.pair.packet.load(Ordering::Relaxed)
                && self.pair.packet_flags.load(Ordering::Relaxed) != 0;
            if has_data || has_packet {
                revents |= PollFlags::In;
            }
        }
        if mask.contains(PollFlags::Out) && has_open_slave(&self.pair) {
            revents |= PollFlags::Out;
        }
        if !has_open_slave(&self.pair) {
            revents |= PollFlags::Hup;
        }
        Ok(revents & (mask | PollFlags::Hup))
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        if mask.intersects(PollFlags::Read) {
            PollEventSet::one(&self.pair.master_rd_event)
        } else {
            PollEventSet::new()
        }
    }
}

impl Drop for PtyMaster {
    fn drop(&mut self) {
        self.pair.tty.hangup();
        self.pair.master_rd_event.wake_all();
        self.pair.master_wr_event.wake_all();

        self.pair.tty.signal_foreground(Signal::SigHup);
        self.pair.tty.signal_foreground(Signal::SigCont);

        PTY_TABLE.lock().remove(&self.pair.index);

        // Best effort unlink. If it fails, we don't care.
        let slave_name = format!("pts/{}", self.pair.index);
        let _ = crate::device::unregister_node(slave_name.as_bytes());
    }
}

pub struct PtmxDevice;

impl CharacterDevice for PtmxDevice {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        let pair = alloc_pty()?;
        Ok(Arc::new(PtyMaster { pair }))
    }

    fn major(&self) -> u32 {
        5
    }

    fn minor(&self) -> u32 {
        2
    }
}

#[initgraph::task(
    name = "generic.device.tty.ptmx",
    depends = [devtmpfs::DEVTMPFS_STAGE],
)]
pub fn PTMX_STAGE() {
    let root = devtmpfs::get_root();

    vfs::mkdir(
        root.clone(),
        root,
        b"pts",
        Mode::from_bits_truncate(0o755),
        Identity::get_kernel(),
    )
    .expect("Unable to create /dev/pts/");

    crate::device::register_char_node(
        b"ptmx",
        Arc::new(PtmxDevice),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create PTMX device");
}
