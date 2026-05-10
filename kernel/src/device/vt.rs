use crate::{
    device::tty::{Tty, TtyDriver},
    posix::errno::EResult,
    uapi::termios::winsize,
    util::mutex::spin::SpinMutex,
    vfs::fs::devtmpfs,
};
use alloc::{
    string::String,
    sync::{Arc, Weak},
};
use core::sync::atomic::{AtomicUsize, Ordering};

pub trait VtDisplay: Send + Sync {
    fn write_output(&self, data: &[u8]);

    fn get_winsize(&self) -> winsize;
}

struct VtManager {
    active: AtomicUsize,
    tty1: Arc<Tty>,
    display: SpinMutex<Option<Arc<dyn VtDisplay>>>,
}

impl VtManager {
    fn write_output(&self, data: &[u8]) {
        if let Some(display) = self.display.lock().as_ref() {
            display.write_output(data);
        }
    }

    fn get_winsize(&self) -> winsize {
        self.display
            .lock()
            .as_ref()
            .map(|display| display.get_winsize())
            .unwrap_or(winsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            })
    }

    fn input_bytes(&self, data: &[u8]) {
        if self.active.load(Ordering::Acquire) != 1 {
            return;
        }

        for &byte in data {
            self.tty1.input_byte(byte);
        }
    }
}

struct VtTtyDriver {
    manager: Weak<VtManager>,
}

impl TtyDriver for VtTtyDriver {
    fn write_output(&self, data: &[u8]) -> EResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager.write_output(data);
        }
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        self.manager
            .upgrade()
            .map(|manager| manager.get_winsize())
            .unwrap_or(winsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            })
    }
}

static VT_MANAGER: SpinMutex<Option<Arc<VtManager>>> = SpinMutex::new(None);

pub fn attach_display(display: Arc<dyn VtDisplay>) {
    let Some(manager) = VT_MANAGER.lock().as_ref().cloned() else {
        return;
    };

    let ws = display.get_winsize();
    *manager.display.lock() = Some(display);
    *manager.tty1.winsize.lock() = ws;
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
        let driver = Arc::new(VtTtyDriver {
            manager: weak.clone(),
        });
        let tty1 = Tty::new(String::from("tty1"), driver);

        VtManager {
            active: AtomicUsize::new(1),
            tty1,
            display: SpinMutex::new(None),
        }
    });

    manager
        .tty1
        .clone()
        .register_device()
        .expect("Unable to create tty1");
    *VT_MANAGER.lock() = Some(manager);
}
