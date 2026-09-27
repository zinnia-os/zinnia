use crate::{
    device::dt::{Node, driver::Driver},
    log::{self, LoggerSink},
    memory::{MmioView, Register, UnsafeMemoryView, VmCacheType},
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};
use alloc::boxed::Box;

struct Device {
    view: SpinMutex<MmioView>,
    reg_shift: u32,
}

/// Transmit holding / receive buffer.
const THR: Register<u8> = Register::new(0);
/// Interrupt enable.
const IER: Register<u8> = Register::new(1);
/// FIFO control.
const FCR: Register<u8> = Register::new(2);
/// Line control.
const LCR: Register<u8> = Register::new(3);
/// Line status.
const LSR: Register<u8> = Register::new(5);
/// Transmit holding register empty.
const LSR_THR_EMPTY: u8 = 0x20;

impl Device {
    fn put_chars(&self, chars: &[u8]) {
        let view = self.view.lock();

        for &ch in chars {
            unsafe {
                while view
                    .read_reg(LSR.shifted(self.reg_shift as usize))
                    .unwrap()
                    .value()
                    & LSR_THR_EMPTY
                    == 0
                {
                    core::hint::spin_loop();
                }
                view.write_reg(THR.shifted(self.reg_shift as usize), ch);
            }
        }
    }
}

static DRIVER: Driver = Driver {
    name: "ns16550a",
    compatible: &[b"ns16550a", b"ns16550"],
    probe,
};

fn probe(node: &Node) -> EResult<()> {
    let (phys, _) = node.reg(0).ok_or(Errno::EINVAL)?;

    let view =
        SpinMutex::new(unsafe { MmioView::new(phys.into(), 0x1000, VmCacheType::Uncacheable) });
    let reg_shift = node.first_cell(b"reg-shift").unwrap_or(0);

    // 8N1, FIFOs on, interrupts off.
    unsafe {
        let locked = view.lock();
        locked.write_reg(IER.shifted(reg_shift as usize), 0x00);
        locked.write_reg(FCR.shifted(reg_shift as usize), 0xC7);
        locked.write_reg(LCR.shifted(reg_shift as usize), 0x03);
    }

    let dev = Device { view, reg_shift };

    log::add_sink(Box::new(dev));
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

impl LoggerSink for Device {
    fn write(&mut self, input: &[u8]) {
        self.put_chars(input);
    }

    fn name(&self) -> &'static str {
        "ns16550a"
    }
}
