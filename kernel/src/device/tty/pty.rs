//! Pseudo-terminal (PTY) support.
//!
//! Opening `/dev/ptmx` allocates a new master/slave pair. The master fd is
//! returned to the caller; the slave appears as `/dev/pts/N`.
//!
//! Data written to the master is fed into the slave's line discipline (as if
//! typed on a keyboard). Data the slave writes (program output) is buffered
//! for the master to read.

use crate::{
    device::tty::{Tty, TtyDriver, TtyFileOps},
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi::{self, termios::winsize},
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{
        self, File,
        file::{FileOps, OpenFlags, PollFlags},
        fs::devtmpfs,
        inode::{Device, Mode},
    },
};
use alloc::{collections::btree_map::BTreeMap, format, sync::Arc, vec};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Counter for PTY indices.
static PTY_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Global table of active PTY pairs (index → master state).
static PTY_TABLE: SpinMutex<BTreeMap<usize, Arc<PtyPair>>> = SpinMutex::new(BTreeMap::new());

pub struct PtyPair {
    /// Index N (for `/dev/pts/N`).
    pub index: usize,
    /// The TTY object associated with the slave side.
    pub tty: Arc<Tty>,
    /// Buffer for slave → master data (program output that the master reads).
    pub master_buf: SpinMutex<RingBuffer>,
    /// Event signalled when data is available for the master to read.
    pub master_rd_event: Event,
    /// Whether the slave is unlocked (accessible via /dev/pts/N).
    pub unlocked: SpinMutex<bool>,
}

struct PtySlaveTtyDriver {
    pair: Arc<PtyPair>,
}

impl TtyDriver for PtySlaveTtyDriver {
    fn write_output(&self, data: &[u8]) {
        self.pair.master_buf.lock().write(data);
        self.pair.master_rd_event.wake_all();
    }

    fn get_winsize(&self) -> winsize {
        *self.pair.tty.winsize.lock()
    }
}

pub struct PtyMaster {
    pub pair: Arc<PtyPair>,
}

impl FileOps for PtyMaster {
    fn read(&self, file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let guard = self.pair.master_rd_event.guard();
        loop {
            {
                let mut mbuf = self.pair.master_buf.lock();
                if !mbuf.is_empty() {
                    let len = core::cmp::min(mbuf.get_data_len(), buffer.len());
                    let mut tmp = vec![0u8; len];
                    let n = mbuf.read(&mut tmp);
                    buffer.copy_from_slice(&tmp[..n])?;
                    return Ok(n as isize);
                }
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if crate::sched::Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn write(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let total = buffer.len();
        let mut data = vec![0u8; total];
        buffer.copy_to_slice(&mut data)?;

        // Feed into the slave's line discipline as if typed on a keyboard.
        for &byte in &data {
            self.pair.tty.input_byte(byte);
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
                *self.pair.unlocked.lock() = lock == 0;
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
        if mask.contains(PollFlags::In) && !self.pair.master_buf.lock().is_empty() {
            revents |= PollFlags::In;
        }
        if mask.contains(PollFlags::Out) {
            revents |= PollFlags::Out;
        }
        Ok(revents)
    }

    fn release(&self, _file: &File) -> EResult<()> {
        // When master is closed, remove from PTY table.
        PTY_TABLE.lock().remove(&self.pair.index);
        Ok(())
    }
}

/// A null driver used as placeholder before the real driver is set.
struct NullTtyDriver;

impl TtyDriver for NullTtyDriver {
    fn write_output(&self, _data: &[u8]) {}
}

/// Allocate a new PTY pair. Returns (master_file_ops, slave_tty).
pub fn alloc_pty() -> EResult<(Arc<PtyMaster>, Arc<Tty>)> {
    let index = PTY_INDEX.fetch_add(1, Ordering::Relaxed);
    let name = format!("pts/{}", index);

    // We need a two-phase init because PtySlaveTtyDriver needs a ref to the pair.
    // Create pair with a placeholder Tty first.
    let pair = Arc::new(PtyPair {
        index,
        tty: Tty::new(name.clone(), Arc::new(NullTtyDriver)),
        master_buf: SpinMutex::new(RingBuffer::new(0x1000)),
        master_rd_event: Event::new(),
        unlocked: SpinMutex::new(false),
    });

    let driver = Arc::new(PtySlaveTtyDriver { pair: pair.clone() });
    let tty = Tty::new(name, driver);

    // Overwrite the placeholder tty in the pair.
    unsafe {
        let pair_ptr = Arc::as_ptr(&pair) as *mut PtyPair;
        core::ptr::write(&raw mut (*pair_ptr).tty, tty.clone());
    }

    PTY_TABLE.lock().insert(index, pair.clone());

    // Register slave as /dev/pts/N.
    tty.clone().register_device().ok();

    let master = Arc::new(PtyMaster { pair });
    Ok((master, tty))
}

pub struct PtmxDevice;

impl FileOps for PtmxDevice {
    fn read(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        // Reads on the ptmx device node itself are invalid.
        Err(Errno::EIO)
    }

    fn write(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        Err(Errno::EIO)
    }

    fn ioctl(&self, _: &File, _: usize, _: VirtAddr) -> EResult<usize> {
        Err(Errno::EIO)
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
        root.clone(),
        b"pts",
        Mode::from_bits_truncate(0o755),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/pts/");

    vfs::mknod(
        root.clone(),
        root,
        b"ptmx",
        Mode::from_bits_truncate(0o666),
        Some(Device::CharacterDevice(Arc::new(PtmxDevice))),
        &Identity::get_kernel(),
    )
    .expect("Unable to create PTMX device");
}
