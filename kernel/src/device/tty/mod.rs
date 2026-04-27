pub mod pty;

use crate::{
    clock,
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::{self, Identity, signal::Signal},
    sched::Scheduler,
    uapi::{self, termios::*},
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{
        self, File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
        fs::devtmpfs,
        inode::{Device, Mode},
    },
};
use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc, vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub trait TtyDriver: Send + Sync {
    fn write_output(&self, data: &[u8]);

    fn get_winsize(&self) -> winsize {
        winsize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

const CANON_BUF_SIZE: usize = 4096;
pub struct LineDiscipline {
    pub termios: termios,
    read_buf: RingBuffer,
    canon_buf: Vec<u8>,
}

use alloc::vec::Vec;

impl LineDiscipline {
    pub fn new() -> Self {
        Self {
            termios: default_termios(),
            read_buf: RingBuffer::new(CANON_BUF_SIZE),
            canon_buf: Vec::new(),
        }
    }

    fn is_canon(&self) -> bool {
        self.termios.c_lflag & ICANON != 0
    }

    fn is_echo(&self) -> bool {
        self.termios.c_lflag & ECHO != 0
    }

    fn is_echoe(&self) -> bool {
        self.termios.c_lflag & ECHOE != 0
    }

    fn is_isig(&self) -> bool {
        self.termios.c_lflag & ISIG != 0
    }

    fn is_icrnl(&self) -> bool {
        self.termios.c_iflag & ICRNL != 0
    }

    fn cc(&self, idx: u32) -> u8 {
        self.termios.c_cc[idx as usize]
    }

    fn append_output(&self, data: &[u8], out: &mut Vec<u8>) {
        if self.termios.c_oflag & OPOST == 0 {
            out.extend_from_slice(data);
            return;
        }

        for &byte in data {
            if byte == b'\n' && self.termios.c_oflag & ONLCR != 0 {
                out.extend_from_slice(b"\r\n");
            } else {
                out.push(byte);
            }
        }
    }

    pub fn input_char(&mut self, mut byte: u8, tty: &Tty) -> Vec<u8> {
        let mut output = Vec::new();
        if self.is_icrnl() && byte == b'\r' {
            byte = b'\n';
        }

        // Signal generation.
        if self.is_isig() {
            if byte == self.cc(VINTR) {
                tty.signal_foreground(Signal::SigInt);
                if self.is_echo() {
                    self.append_output(b"^C\n", &mut output);
                }
                return output;
            }
            if byte == self.cc(VQUIT) {
                tty.signal_foreground(Signal::SigQuit);
                if self.is_echo() {
                    self.append_output(b"^\\\n", &mut output);
                }
                return output;
            }
            if byte == self.cc(VSUSP) {
                tty.signal_foreground(Signal::SigTstp);
                if self.is_echo() {
                    self.append_output(b"^Z\n", &mut output);
                }
                return output;
            }
        }

        if self.is_canon() {
            self.input_canon(byte, tty, &mut output);
        } else {
            self.input_raw(byte, tty, &mut output);
        }
        output
    }

    fn input_canon(&mut self, byte: u8, tty: &Tty, output: &mut Vec<u8>) {
        if byte == self.cc(VERASE) {
            if let Some(_) = self.canon_buf.pop() {
                if self.is_echo() && self.is_echoe() {
                    self.append_output(b"\x08 \x08", output);
                }
            }
            return;
        }

        if byte == self.cc(VKILL) {
            if self.is_echo() {
                for _ in 0..self.canon_buf.len() {
                    self.append_output(b"\x08 \x08", output);
                }
            }
            self.canon_buf.clear();
            return;
        }

        if byte == self.cc(VEOF) {
            self.read_buf.write(&self.canon_buf);
            self.canon_buf.clear();
            tty.rd_event.wake_all();
            return;
        }

        if self.canon_buf.len() < CANON_BUF_SIZE {
            self.canon_buf.push(byte);
        }

        if self.is_echo() {
            self.append_output(&[byte], output);
        }

        if byte == b'\n' || byte == self.cc(VEOL) {
            self.read_buf.write(&self.canon_buf);
            self.canon_buf.clear();
            tty.rd_event.wake_all();
        }
    }

    fn input_raw(&mut self, byte: u8, tty: &Tty, output: &mut Vec<u8>) {
        self.read_buf.write(&[byte]);
        if self.is_echo() {
            self.append_output(&[byte], output);
        }
        tty.rd_event.wake_all();
    }

    pub fn read_available(&self) -> usize {
        self.read_buf.get_data_len()
    }

    pub fn read_into(&mut self, buf: &mut [u8]) -> usize {
        self.read_buf.read(buf)
    }

    pub fn write_output(&self, data: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len());
        self.append_output(data, &mut output);
        output
    }
}

fn default_termios() -> termios {
    let mut t = termios::default();
    t.c_iflag = ICRNL | IXON;
    t.c_oflag = OPOST | ONLCR;
    t.c_cflag = CS8 | CREAD;
    t.c_lflag = ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHONL | IEXTEN;

    t.c_cc[VINTR as usize] = 0x03; // ^C
    t.c_cc[VQUIT as usize] = 0x1C; // ^\
    t.c_cc[VERASE as usize] = 0x7F; // DEL
    t.c_cc[VKILL as usize] = 0x15; // ^U
    t.c_cc[VEOF as usize] = 0x04; // ^D
    t.c_cc[VTIME as usize] = 0;
    t.c_cc[VMIN as usize] = 1;
    t.c_cc[VSTART as usize] = 0x11; // ^Q
    t.c_cc[VSTOP as usize] = 0x13; // ^S
    t.c_cc[VSUSP as usize] = 0x1A; // ^Z
    t.c_cc[VEOL as usize] = 0;
    t.c_cc[VREPRINT as usize] = 0x12; // ^R
    t.c_cc[VDISCARD as usize] = 0x0F; // ^O
    t.c_cc[VWERASE as usize] = 0x17; // ^W
    t.c_cc[VLNEXT as usize] = 0x16; // ^V
    t
}

static TTY_INDEX: AtomicUsize = AtomicUsize::new(0);
static TTYS: SpinMutex<BTreeMap<usize, Arc<Tty>>> = SpinMutex::new(BTreeMap::new());

pub struct Tty {
    pub name: String,
    pub index: usize,
    pub ldisc: SpinMutex<LineDiscipline>,
    pub driver: Arc<dyn TtyDriver>,
    pub winsize: SpinMutex<winsize>,
    pub foreground_pgrp: SpinMutex<Option<uapi::pid_t>>,
    pub session: SpinMutex<Option<uapi::pid_t>>,
    pub rd_event: Event,
    pub hangup: AtomicBool,
}

impl Tty {
    /// Create a new TTY backed by the given driver and register it globally.
    pub fn new(name: String, driver: Arc<dyn TtyDriver>) -> Arc<Self> {
        let ws = driver.get_winsize();
        let index = TTY_INDEX.fetch_add(1, Ordering::Relaxed);
        let tty = Arc::new(Self {
            name,
            index,
            ldisc: SpinMutex::new(LineDiscipline::new()),
            driver,
            winsize: SpinMutex::new(ws),
            foreground_pgrp: SpinMutex::new(None),
            session: SpinMutex::new(None),
            rd_event: Event::new(),
            hangup: AtomicBool::new(false),
        });
        TTYS.lock().insert(index, tty.clone());
        tty
    }

    /// Mark the TTY as hung up and wake all blocked readers.
    /// Subsequent writes return EIO and reads return EOF once the ldisc buffer is drained.
    pub fn hangup(&self) {
        self.hangup.store(true, Ordering::Release);
        self.rd_event.wake_all();
    }

    pub fn is_hung_up(&self) -> bool {
        self.hangup.load(Ordering::Acquire)
    }

    /// Feed a byte from the hardware into the line discipline.
    /// Typically called from an IRQ handler.
    pub fn input_byte(&self, byte: u8) {
        let output = self.input_byte_internal(byte);
        if !output.is_empty() {
            self.driver.write_output(&output);
        }
    }

    pub fn input_byte_internal(&self, byte: u8) -> Vec<u8> {
        self.ldisc.lock().input_char(byte, self)
    }

    /// Send a signal to the foreground process group, if any.
    pub fn signal_foreground(&self, sig: Signal) {
        if let Some(pgrp) = *self.foreground_pgrp.lock() {
            process::signal::send_signal_to_pgrp(pgrp, sig);
        }
    }

    /// Register this TTY as a character device under `/dev/{name}`.
    pub fn register_device(self: Arc<Self>) -> EResult<()> {
        let ops: Arc<dyn FileOps> = Arc::new(TtyFileOps { tty: self.clone() });
        self.register_device_with_ops(ops)
    }

    pub fn register_device_with_ops(self: Arc<Self>, ops: Arc<dyn FileOps>) -> EResult<()> {
        let dev_name = self.as_ref().name.as_bytes();
        let root = devtmpfs::get_root();

        vfs::mknod(
            root.clone(),
            root,
            dev_name,
            Mode::from_bits_truncate(0o666),
            Some(Device::CharacterDevice(ops)),
            &Identity::get_kernel(),
        )
    }
}

pub fn get_tty(index: usize) -> Option<Arc<Tty>> {
    TTYS.lock().get(&index).cloned()
}

pub fn get_tty_by_name(name: &str) -> Option<Arc<Tty>> {
    TTYS.lock().values().find(|t| t.name == name).cloned()
}

pub struct TtyFileOps {
    pub tty: Arc<Tty>,
}

impl FileOps for TtyFileOps {
    fn read(&self, file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        // Non-blocking: return data immediately or EAGAIN.
        if file.flags.lock().contains(OpenFlags::NonBlocking) {
            let mut ldisc = self.tty.ldisc.lock();
            let avail = ldisc.read_available();
            if avail == 0 {
                if self.tty.is_hung_up() {
                    return Ok(0);
                }
                return Err(Errno::EAGAIN);
            }

            let len = core::cmp::min(avail, buffer.len());
            let mut tmp = vec![0u8; len];
            let n = ldisc.read_into(&mut tmp);
            buffer.copy_from_slice(&tmp[..n])?;
            return Ok(n as isize);
        }

        // Snapshot termios parameters (they shouldn't change mid-read).
        let (canon, vmin, vtime) = {
            let ldisc = self.tty.ldisc.lock();
            (
                ldisc.is_canon(),
                ldisc.cc(VMIN) as usize,
                ldisc.cc(VTIME) as usize,
            )
        };

        if canon {
            let mut woken = false;
            loop {
                let guard = self.tty.rd_event.guard();
                let mut ldisc = self.tty.ldisc.lock();
                let avail = ldisc.read_available();

                if avail > 0 {
                    let len = core::cmp::min(avail, buffer.len());
                    let mut tmp = vec![0u8; len];
                    let n = ldisc.read_into(&mut tmp);
                    buffer.copy_from_slice(&tmp[..n])?;
                    return Ok(n as isize);
                }

                if self.tty.is_hung_up() {
                    return Ok(0); // EOF, other end hung up.
                }

                if woken {
                    return Ok(0); // EOF, VEOF with empty line.
                }

                drop(ldisc);
                guard.wait();
                if crate::sched::Scheduler::get_current().has_pending_signals() {
                    return Err(crate::posix::errno::Errno::EINTR);
                }
                woken = true;
            }
        } else if vmin == 0 && vtime == 0 {
            let mut ldisc = self.tty.ldisc.lock();
            let avail = ldisc.read_available();
            if avail == 0 {
                return Ok(0);
            }
            let len = core::cmp::min(avail, buffer.len());
            let mut tmp = vec![0u8; len];
            let n = ldisc.read_into(&mut tmp);
            buffer.copy_from_slice(&tmp[..n])?;
            Ok(n as isize)
        } else if vmin > 0 && vtime == 0 {
            let target = core::cmp::min(vmin, buffer.len());
            loop {
                let guard = self.tty.rd_event.guard();
                let mut ldisc = self.tty.ldisc.lock();
                let avail = ldisc.read_available();

                if avail >= target {
                    let len = core::cmp::min(avail, buffer.len());
                    let mut tmp = vec![0u8; len];
                    let n = ldisc.read_into(&mut tmp);
                    buffer.copy_from_slice(&tmp[..n])?;
                    return Ok(n as isize);
                }

                if self.tty.is_hung_up() {
                    // Return whatever is buffered (possibly 0) as EOF.
                    let len = core::cmp::min(avail, buffer.len());
                    if len > 0 {
                        let mut tmp = vec![0u8; len];
                        let n = ldisc.read_into(&mut tmp);
                        buffer.copy_from_slice(&tmp[..n])?;
                        return Ok(n as isize);
                    }
                    return Ok(0);
                }

                drop(ldisc);
                guard.wait();
                if crate::sched::Scheduler::get_current().has_pending_signals() {
                    return Err(crate::posix::errno::Errno::EINTR);
                }
            }
        } else if vmin == 0 && vtime > 0 {
            // Raw, MIN=0 TIME>0: wait for 1 char or timeout (VTIME × 100ms).
            let deadline = clock::get_elapsed() + vtime * 100_000_000;
            loop {
                let guard = self.tty.rd_event.guard();
                let mut ldisc = self.tty.ldisc.lock();
                let avail = ldisc.read_available();

                if avail > 0 {
                    let len = core::cmp::min(avail, buffer.len());
                    let mut tmp = vec![0u8; len];
                    let n = ldisc.read_into(&mut tmp);
                    buffer.copy_from_slice(&tmp[..n])?;
                    return Ok(n as isize);
                }

                if self.tty.is_hung_up() || clock::get_elapsed() >= deadline {
                    return Ok(0); // Timer expired or hangup, no data.
                }

                drop(ldisc);
                guard.wait();
                if crate::sched::Scheduler::get_current().has_pending_signals() {
                    return Err(crate::posix::errno::Errno::EINTR);
                }
            }
        } else {
            // Raw, MIN>0 TIME>0: inter-character timer.
            // Block for first byte, then start timer; read until min(VMIN, requested)
            // bytes or timer expires after each byte.
            let target = core::cmp::min(vmin, buffer.len());
            let mut bytes_read = 0usize;
            let mut deadline: Option<usize> = None;

            loop {
                let guard = self.tty.rd_event.guard();
                let mut ldisc = self.tty.ldisc.lock();
                let avail = ldisc.read_available();

                if avail > 0 {
                    let len = core::cmp::min(avail, buffer.len());
                    let mut tmp = vec![0u8; len];
                    let n = ldisc.read_into(&mut tmp);
                    buffer.copy_from_slice(&tmp[..n])?;
                    bytes_read += n;

                    if bytes_read >= target {
                        return Ok(bytes_read as isize);
                    }

                    // (Re)start inter-character timer.
                    deadline = Some(clock::get_elapsed() + vtime * 100_000_000);
                }

                if let Some(dl) = deadline {
                    if clock::get_elapsed() >= dl {
                        return Ok(bytes_read as isize);
                    }
                }

                if self.tty.is_hung_up() {
                    return Ok(bytes_read as isize);
                }

                drop(ldisc);
                guard.wait();
                if crate::sched::Scheduler::get_current().has_pending_signals() {
                    if bytes_read > 0 {
                        return Ok(bytes_read as isize);
                    }
                    return Err(crate::posix::errno::Errno::EINTR);
                }
            }
        }
    }

    fn write(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        if self.tty.is_hung_up() {
            return Err(Errno::EIO);
        }

        let total = buffer.len();
        let mut data = vec![0u8; total];
        buffer.copy_to_slice(&mut data)?;

        let output = self.tty.ldisc.lock().write_output(&data);
        self.tty.driver.write_output(&output);

        Ok(total as isize)
    }

    fn ioctl(&self, _file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        match request as u32 {
            uapi::ioctls::TCGETS => {
                let ldisc = self.tty.ldisc.lock();
                let mut ptr = UserPtr::new(arg);
                ptr.write(ldisc.termios).ok_or(Errno::EFAULT)?;
            }
            uapi::ioctls::TCSETS | uapi::ioctls::TCSETSW | uapi::ioctls::TCSETSF => {
                let ptr: UserPtr<termios> = UserPtr::new(arg);
                let new_termios = ptr.read().ok_or(Errno::EFAULT)?;
                self.tty.ldisc.lock().termios = new_termios;
            }
            uapi::ioctls::TIOCGWINSZ => {
                let mut ptr = UserPtr::new(arg);
                let ws = *self.tty.winsize.lock();
                ptr.write(ws).ok_or(Errno::EFAULT)?;
            }
            uapi::ioctls::TIOCSWINSZ => {
                let ptr: UserPtr<winsize> = UserPtr::new(arg);
                let ws = ptr.read().ok_or(Errno::EFAULT)?;
                let changed = {
                    let mut cur = self.tty.winsize.lock();
                    let changed = cur.ws_row != ws.ws_row
                        || cur.ws_col != ws.ws_col
                        || cur.ws_xpixel != ws.ws_xpixel
                        || cur.ws_ypixel != ws.ws_ypixel;
                    *cur = ws;
                    changed
                };
                if changed {
                    self.tty.signal_foreground(Signal::SigWinch);
                }
            }
            uapi::ioctls::TIOCGPGRP => {
                let pgrp = self.tty.foreground_pgrp.lock().unwrap_or(0);
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(pgrp as i32).ok_or(Errno::EFAULT)?;
            }
            uapi::ioctls::TIOCSPGRP => {
                let ptr: UserPtr<i32> = UserPtr::new(arg);
                let pgrp = ptr.read().ok_or(Errno::EFAULT)?;
                *self.tty.foreground_pgrp.lock() = Some(pgrp);
            }
            uapi::ioctls::TIOCSCTTY => {
                let proc = Scheduler::get_current().get_process();
                *self.tty.session.lock() = Some(proc.get_pid());
                *self.tty.foreground_pgrp.lock() = Some(*proc.pgrp.lock());
                *proc.controlling_tty.lock() = Some(self.tty.clone());
            }
            uapi::ioctls::TIOCNOTTY => {
                let proc = Scheduler::get_current().get_process();
                *proc.controlling_tty.lock() = None;
            }
            uapi::ioctls::TIOCGNAME => {
                let name = self.tty.name.as_bytes();
                let ptr: UserPtr<u8> = UserPtr::new(arg);
                // Write the name + NUL terminator.
                for (i, &b) in name.iter().chain(core::iter::once(&0u8)).enumerate() {
                    let mut p: UserPtr<u8> = UserPtr::new(arg + i);
                    p.write(b).ok_or(Errno::EFAULT)?;
                }
                let _ = ptr;
            }
            uapi::ioctls::TIOCGSID => {
                let sid = self.tty.session.lock().unwrap_or(0);
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(sid as i32).ok_or(Errno::EFAULT)?;
            }
            uapi::ioctls::FIONREAD => {
                let ldisc = self.tty.ldisc.lock();
                let avail = ldisc.read_available() as i32;
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(avail).ok_or(Errno::EFAULT)?;
            }
            _ => return Err(Errno::ENOTTY),
        }
        Ok(0)
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let mut revents = PollFlags::empty();
        if mask.contains(PollFlags::In) {
            let ldisc = self.tty.ldisc.lock();
            if ldisc.read_available() > 0 {
                revents |= PollFlags::In;
            }
        }
        if mask.contains(PollFlags::Out) {
            revents |= PollFlags::Out; // TTYs are always writable.
        }
        Ok(revents)
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        if mask.intersects(PollFlags::Read) {
            PollEventSet::one(&self.tty.rd_event)
        } else {
            PollEventSet::new()
        }
    }
}

struct CttyFileOps;

impl FileOps for CttyFileOps {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let tty = get_controlling_tty()?;
        let ops = TtyFileOps { tty };
        ops.read(file, buffer, offset)
    }

    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let tty = get_controlling_tty()?;
        let ops = TtyFileOps { tty };
        ops.write(file, buffer, offset)
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        let tty = get_controlling_tty()?;
        let ops = TtyFileOps { tty };
        ops.ioctl(file, request, arg)
    }

    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let tty = get_controlling_tty()?;
        let ops = TtyFileOps { tty };
        ops.poll(file, mask)
    }
}

fn get_controlling_tty() -> EResult<Arc<Tty>> {
    let proc = Scheduler::get_current().get_process();
    proc.controlling_tty.lock().clone().ok_or(Errno::ENXIO)
}

#[initgraph::task(
    name = "generic.device.tty.ctty",
    depends = [devtmpfs::DEVTMPFS_STAGE],
)]
pub fn CTTY_STAGE() {
    let root = devtmpfs::get_root();

    vfs::mknod(
        root.clone(),
        root,
        b"tty",
        Mode::from_bits_truncate(0o660),
        Some(Device::CharacterDevice(Arc::new(CttyFileOps))),
        &Identity::get_kernel(),
    )
    .expect("Unable to register ctty device");
}
