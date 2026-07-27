use crate::{
    device::dt::{Node, driver::Driver},
    device::tty::{Tty, TtyDriver},
    log::{self, LoggerSink},
    memory::{
        PhysAddr,
        pmm::KernelAlloc,
        virt::{VmCacheType, VmFlags, mmu::PageTable},
    },
    posix::errno::{EResult, Errno},
};
use alloc::{boxed::Box, string::String, sync::Arc};
use core::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};

/// Transmit holding / receive buffer.
const THR: usize = 0;
/// Interrupt enable.
const IER: usize = 1;
/// FIFO control.
const FCR: usize = 2;
/// Line control.
const LCR: usize = 3;
/// Line status.
const LSR: usize = 5;
/// Transmit holding register empty.
const LSR_THR_EMPTY: u8 = 0x20;

static BASE: AtomicPtr<u8> = AtomicPtr::new(null_mut());
/// Register stride, as the `reg-shift` cell count from the device tree.
static REG_SHIFT: AtomicU32 = AtomicU32::new(0);

unsafe fn reg_ptr(base: *mut u8, index: usize) -> *mut u8 {
    unsafe { base.add(index << REG_SHIFT.load(Ordering::Relaxed)) }
}

unsafe fn put(base: *mut u8, ch: u8) {
    unsafe {
        while reg_ptr(base, LSR).read_volatile() & LSR_THR_EMPTY == 0 {
            core::hint::spin_loop();
        }
        reg_ptr(base, THR).write_volatile(ch);
    }
}

struct Ns16550Logger;

impl LoggerSink for Ns16550Logger {
    fn write(&mut self, input: &[u8]) {
        let base = BASE.load(Ordering::Relaxed);
        if base.is_null() {
            return;
        }
        for &ch in input {
            unsafe { put(base, ch) };
            if ch == b'\n' {
                unsafe { put(base, b'\r') };
            }
        }
    }

    fn name(&self) -> &'static str {
        "ttyS0"
    }
}

static DRIVER: Driver = Driver {
    name: "ns16550a",
    compatible: &[b"ns16550a", b"ns16550"],
    probe,
};

fn probe(node: &Node) -> EResult<()> {
    let (phys, _) = node.reg(0).ok_or(Errno::EINVAL)?;
    let base = PageTable::get_kernel()
        .map_memory::<KernelAlloc>(
            PhysAddr::from(phys as usize),
            VmFlags::Read | VmFlags::Write,
            VmCacheType::Uncacheable,
            0x1000,
        )
        .map_err(|_| Errno::ENOMEM)?;
    REG_SHIFT.store(node.first_cell(b"reg-shift", 0), Ordering::Relaxed);
    BASE.store(base, Ordering::Relaxed);

    // 8N1, FIFOs on, interrupts off.
    unsafe {
        reg_ptr(base, IER).write_volatile(0x00);
        reg_ptr(base, FCR).write_volatile(0xC7);
        reg_ptr(base, LCR).write_volatile(0x03);
    }

    log::add_sink(Box::new(Ns16550Logger));
    Ok(())
}

#[task(
    name = "device.serial.ns16550a",
    depends = [crate::device::dt::TREE_STAGE],
)]
fn SERIAL_STAGE() {
    if let Err(err) = DRIVER.register() {
        warn!("Failed to register the ns16550a console: {err:?}");
    }
}

struct Ns16550TtyDriver;

impl TtyDriver for Ns16550TtyDriver {
    fn write_output(&self, data: &[u8]) -> EResult<()> {
        let base = BASE.load(Ordering::Relaxed);
        if base.is_null() {
            return Err(Errno::ENODEV);
        }
        for &ch in data {
            unsafe { put(base, ch) };
        }
        Ok(())
    }
}

#[task(
    name = "device.serial.ns16550a_file",
    depends = [
        crate::vfs::VFS_STAGE,
        crate::vfs::fs::devtmpfs::DEVTMPFS_STAGE,
        SERIAL_STAGE,
    ],
)]
fn SERIAL_FILE_STAGE() {
    let base = BASE.load(Ordering::Relaxed);
    if base.is_null() {
        return;
    }

    // TODO: wire an RX interrupt via `arch::irq::map_dt_interrupt`.
    let tty = Tty::new(String::from("ttyS0"), Arc::new(Ns16550TtyDriver));
    tty.register_device().expect("Unable to create ttyS0");
}
