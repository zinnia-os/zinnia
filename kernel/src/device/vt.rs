use crate::{
    device::{
        self,
        tty::{Tty, TtyDriver, TtyFileOps},
    },
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::{
        PROCESS_TABLE,
        signal::{Signal, send_signal_to_process},
    },
    sched::Scheduler,
    uapi::{ioctls, pid_t, termios::winsize},
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::{
        File,
        file::{FileOps, PollEventSet, PollFlags},
        fs::devtmpfs,
        inode::Mode,
    },
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Number of virtual terminals.
const NUM_VTS: usize = 6;

fn default_winsize() -> winsize {
    winsize {
        ws_row: 25,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

pub trait VtDisplay: Send + Sync {
    fn write_output(&self, data: &[u8]);

    fn get_winsize(&self) -> winsize;

    /// Repaint the whole console.
    fn refresh(&self);
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VtModeIoctl {
    mode: u8,
    waitv: u8,
    relsig: i16,
    acqsig: i16,
    frsig: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VtStat {
    v_active: u16,
    v_signal: u16,
    v_state: u16,
}

struct VtModeState {
    process: bool,
    relsig: i16,
    acqsig: i16,
    owner: Option<pid_t>,
}

impl VtModeState {
    const fn new() -> Self {
        Self {
            process: false,
            relsig: 0,
            acqsig: 0,
            owner: None,
        }
    }
}

pub struct Vt {
    number: usize,
    tty: Arc<Tty>,
    kd_mode: AtomicU32,
    kb_mode: AtomicU32,
    mode: SpinMutex<VtModeState>,
}

struct VtManager {
    active: AtomicUsize,
    vts: Vec<Arc<Vt>>,
    display: SpinMutex<Option<Arc<dyn VtDisplay>>>,
    switch_pending: SpinMutex<Option<usize>>,
    switch_event: Event,
}

fn signal_pid(pid: pid_t, sig: Signal) -> bool {
    let proc = PROCESS_TABLE.lock().get(&pid).and_then(Weak::upgrade);
    match proc {
        Some(proc) => send_signal_to_process(&proc, sig),
        None => false,
    }
}

impl VtManager {
    fn vt(&self, number: usize) -> Option<&Arc<Vt>> {
        if number == 0 {
            return None;
        }
        self.vts.get(number - 1)
    }

    fn active_number(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    fn active_vt(&self) -> &Arc<Vt> {
        self.vt(self.active_number()).unwrap_or(&self.vts[0])
    }

    fn write_from_vt(&self, number: usize, data: &[u8]) {
        if number != self.active_number() {
            return;
        }
        let Some(vt) = self.vt(number) else {
            return;
        };
        if vt.kd_mode.load(Ordering::Acquire) != ioctls::KD_TEXT {
            return;
        }
        if let Some(display) = self.display.lock().as_ref() {
            display.write_output(data);
        }
    }

    fn get_winsize(&self) -> winsize {
        self.display
            .lock()
            .as_ref()
            .map(|display| display.get_winsize())
            .unwrap_or_else(default_winsize)
    }

    fn input_bytes(&self, data: &[u8]) {
        let Some(vt) = self.vt(self.active_number()) else {
            return;
        };
        if vt.kd_mode.load(Ordering::Acquire) == ioctls::KD_GRAPHICS {
            return;
        }
        let kb = vt.kb_mode.load(Ordering::Acquire);
        if kb != ioctls::K_XLATE && kb != ioctls::K_UNICODE {
            return;
        }
        for &byte in data {
            vt.tty.input_byte(byte);
        }
    }

    fn refresh_if_text(&self, vt: &Vt) {
        if vt.kd_mode.load(Ordering::Acquire) == ioctls::KD_TEXT {
            if let Some(display) = self.display.lock().as_ref() {
                display.refresh();
            }
        }
    }

    fn activate(&self, target: usize) -> EResult<usize> {
        if self.vt(target).is_none() {
            return Err(Errno::EINVAL);
        }
        let current = self.active_number();
        if current == target {
            self.switch_event.wake_all();
            return Ok(0);
        }

        let (process, relsig, owner) = {
            let cur = self.vt(current).unwrap();
            let mode = cur.mode.lock();
            (mode.process, mode.relsig, mode.owner)
        };

        if process {
            if let (Some(pid), Ok(sig)) = (owner, Signal::try_from(relsig as u32)) {
                *self.switch_pending.lock() = Some(target);
                if signal_pid(pid, sig) {
                    return Ok(0);
                }
                // The controlling process is gone, switch immediately.
                *self.switch_pending.lock() = None;
            }
        }

        self.finish_switch(target);
        Ok(0)
    }

    fn finish_switch(&self, target: usize) {
        let Some(vt) = self.vt(target).cloned() else {
            return;
        };
        self.active.store(target, Ordering::Release);
        *self.switch_pending.lock() = None;

        self.refresh_if_text(&vt);

        let (process, acqsig, owner) = {
            let mode = vt.mode.lock();
            (mode.process, mode.acqsig, mode.owner)
        };
        if process {
            if let (Some(pid), Ok(sig)) = (owner, Signal::try_from(acqsig as u32)) {
                signal_pid(pid, sig);
            }
        }

        self.switch_event.wake_all();
    }

    fn reldisp(&self, arg: usize) -> EResult<usize> {
        let target = {
            let mut pending = self.switch_pending.lock();
            match *pending {
                Some(_) if arg == 0 => {
                    *pending = None;
                    None
                }
                Some(target) => Some(target),
                None => None,
            }
        };
        if let Some(target) = target {
            self.finish_switch(target);
        }
        Ok(0)
    }

    fn wait_active(&self, target: usize) -> EResult<usize> {
        if self.vt(target).is_none() {
            return Err(Errno::EINVAL);
        }
        loop {
            let guard = self.switch_event.guard();
            if self.active_number() == target {
                return Ok(0);
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn open_query(&self) -> usize {
        for vt in &self.vts {
            if vt.mode.lock().owner.is_none() {
                return vt.number;
            }
        }
        self.vts.len() + 1
    }

    fn state_mask(&self) -> u16 {
        let mut mask = 0u16;
        for vt in &self.vts {
            if vt.number < 16 {
                mask |= 1 << vt.number;
            }
        }
        mask
    }

    fn vt_ioctl(&self, vt: &Arc<Vt>, request: u32, arg: VirtAddr) -> EResult<Option<usize>> {
        match request {
            ioctls::KDGETMODE => {
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(vt.kd_mode.load(Ordering::Acquire) as i32)
                    .ok_or(Errno::EFAULT)?;
            }
            ioctls::KDSETMODE => {
                let value = arg.value() as u32;
                vt.kd_mode.store(value, Ordering::Release);
                if vt.number == self.active_number() {
                    self.refresh_if_text(vt);
                }
            }
            ioctls::KDGKBMODE => {
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(vt.kb_mode.load(Ordering::Acquire) as i32)
                    .ok_or(Errno::EFAULT)?;
            }
            ioctls::KDSKBMODE => {
                vt.kb_mode.store(arg.value() as u32, Ordering::Release);
            }
            ioctls::VT_GETMODE => {
                let state = vt.mode.lock();
                let value = VtModeIoctl {
                    mode: if state.process {
                        ioctls::VT_PROCESS
                    } else {
                        ioctls::VT_AUTO
                    },
                    waitv: 0,
                    relsig: state.relsig,
                    acqsig: state.acqsig,
                    frsig: 0,
                };
                let mut ptr: UserPtr<VtModeIoctl> = UserPtr::new(arg);
                ptr.write(value).ok_or(Errno::EFAULT)?;
            }
            ioctls::VT_SETMODE => {
                let ptr: UserPtr<VtModeIoctl> = UserPtr::new(arg);
                let value = ptr.read().ok_or(Errno::EFAULT)?;
                let mut state = vt.mode.lock();
                if value.mode == ioctls::VT_PROCESS {
                    state.process = true;
                    state.relsig = value.relsig;
                    state.acqsig = value.acqsig;
                    state.owner = Some(Scheduler::get_current().get_process().get_pid());
                } else {
                    *state = VtModeState::new();
                }
            }
            ioctls::VT_GETSTATE => {
                let value = VtStat {
                    v_active: self.active_number() as u16,
                    v_signal: 0,
                    v_state: self.state_mask(),
                };
                let mut ptr: UserPtr<VtStat> = UserPtr::new(arg);
                ptr.write(value).ok_or(Errno::EFAULT)?;
            }
            ioctls::VT_OPENQRY => {
                let mut ptr: UserPtr<i32> = UserPtr::new(arg);
                ptr.write(self.open_query() as i32).ok_or(Errno::EFAULT)?;
            }
            ioctls::VT_ACTIVATE => return self.activate(arg.value()).map(Some),
            ioctls::VT_WAITACTIVE => return self.wait_active(arg.value()).map(Some),
            ioctls::VT_RELDISP => return self.reldisp(arg.value()).map(Some),
            ioctls::VT_DISALLOCATE => {}
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}

struct VtTtyDriver {
    manager: Weak<VtManager>,
    number: usize,
}

impl TtyDriver for VtTtyDriver {
    fn write_output(&self, data: &[u8]) -> EResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager.write_from_vt(self.number, data);
        }
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        self.manager
            .upgrade()
            .map(|manager| manager.get_winsize())
            .unwrap_or_else(default_winsize)
    }
}

/// Per-VT character device.
struct VtFileOps {
    manager: Arc<VtManager>,
    vt: Arc<Vt>,
}

impl VtFileOps {
    fn tty_ops(&self) -> TtyFileOps {
        TtyFileOps {
            tty: self.vt.tty.clone(),
        }
    }
}

impl FileOps for VtFileOps {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        self.tty_ops().read(file, buffer, offset)
    }

    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        self.tty_ops().write(file, buffer, offset)
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        if let Some(ret) = self.manager.vt_ioctl(&self.vt, request as u32, arg)? {
            return Ok(ret);
        }
        self.tty_ops().ioctl(file, request, arg)
    }

    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        self.tty_ops().poll(file, mask)
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        if mask.intersects(PollFlags::Read) {
            PollEventSet::one(&self.vt.tty.rd_event)
        } else {
            PollEventSet::new()
        }
    }
}

/// The control terminal of the currently active VT.
struct Tty0FileOps {
    manager: Arc<VtManager>,
}

impl Tty0FileOps {
    fn active_ops(&self) -> TtyFileOps {
        TtyFileOps {
            tty: self.manager.active_vt().tty.clone(),
        }
    }
}

impl FileOps for Tty0FileOps {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        self.active_ops().read(file, buffer, offset)
    }

    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        self.active_ops().write(file, buffer, offset)
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        let active = self.manager.active_vt().clone();
        if let Some(ret) = self.manager.vt_ioctl(&active, request as u32, arg)? {
            return Ok(ret);
        }
        self.active_ops().ioctl(file, request, arg)
    }

    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        self.active_ops().poll(file, mask)
    }
}

static VT_MANAGER: SpinMutex<Option<Arc<VtManager>>> = SpinMutex::new(None);

pub fn attach_display(display: Arc<dyn VtDisplay>) {
    let Some(manager) = VT_MANAGER.lock().as_ref().cloned() else {
        return;
    };

    let ws = display.get_winsize();
    *manager.display.lock() = Some(display);
    for vt in &manager.vts {
        *vt.tty.winsize.lock() = ws;
    }
}

pub fn input_bytes(data: &[u8]) {
    if let Some(manager) = VT_MANAGER.lock().as_ref().cloned() {
        manager.input_bytes(data);
    }
}

#[initgraph::task(
    name = "generic.device.vt",
    depends = [devtmpfs::DEVTMPFS_STAGE],
)]
pub fn VT_STAGE() {
    let manager = Arc::new_cyclic(|weak| {
        let mut vts = Vec::with_capacity(NUM_VTS);
        for number in 1..=NUM_VTS {
            let driver = Arc::new(VtTtyDriver {
                manager: weak.clone(),
                number,
            });
            let tty = Tty::new(format!("tty{number}"), driver);
            vts.push(Arc::new(Vt {
                number,
                tty,
                kd_mode: AtomicU32::new(ioctls::KD_TEXT),
                kb_mode: AtomicU32::new(ioctls::K_XLATE),
                mode: SpinMutex::new(VtModeState::new()),
            }));
        }

        VtManager {
            active: AtomicUsize::new(1),
            vts,
            display: SpinMutex::new(None),
            switch_pending: SpinMutex::new(None),
            switch_event: Event::new(),
        }
    });

    for vt in &manager.vts {
        let ops: Arc<dyn FileOps> = Arc::new(VtFileOps {
            manager: manager.clone(),
            vt: vt.clone(),
        });
        vt.tty
            .clone()
            .register_device_with_ops(ops)
            .expect("Unable to register VT tty");
    }

    let tty0: Arc<dyn FileOps> = Arc::new(Tty0FileOps {
        manager: manager.clone(),
    });
    device::register_char_node(
        b"tty0",
        device::make_shared(tty0, 4, 0),
        Mode::from_bits_truncate(0o620),
    )
    .expect("Unable to register tty0");

    *VT_MANAGER.lock() = Some(manager);
}
