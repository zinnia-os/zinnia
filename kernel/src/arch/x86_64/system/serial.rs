use crate::{
    arch::x86_64::asm::{read8, write8},
    device::tty::{Tty, TtyDriver},
    irq::{IrqHandler, IrqLine},
    log::{self, LoggerSink},
    uapi::termios::winsize,
};
use alloc::{boxed::Box, string::String, sync::Arc};

/// Serial port
pub const COM1_BASE: u16 = 0x3F8;
/// Data Register
pub const DATA_REG: u16 = 0;
/// Line Status Register
const LSR_REG: u16 = 5;
/// Received data available bit
const LSR_DATA_READY: u8 = 0x01;
/// Transmitter holding register empty bit
const LSR_THR_EMPTY: u8 = 0x20;

struct SerialLogger;

impl SerialLogger {
    fn is_tx_ready() -> bool {
        unsafe { read8(COM1_BASE + LSR_REG) & LSR_THR_EMPTY != 0 }
    }
}

impl LoggerSink for SerialLogger {
    fn write(&mut self, input: &[u8]) {
        for ch in input {
            while !Self::is_tx_ready() {
                core::hint::spin_loop();
            }

            unsafe { write8(COM1_BASE + DATA_REG, *ch) };

            // Most consoles expect a carriage return after a newline.
            if *ch == b'\n' {
                unsafe { write8(COM1_BASE + DATA_REG, b'\r') };
            }
        }
    }

    fn name(&self) -> &'static str {
        "com1"
    }
}

struct SerialTtyDriver;

impl TtyDriver for SerialTtyDriver {
    fn write_output(&self, data: &[u8]) {
        for &ch in data {
            while !SerialLogger::is_tx_ready() {
                core::hint::spin_loop();
            }
            unsafe { write8(COM1_BASE + DATA_REG, ch) };
        }
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

struct SerialIrqHandler {
    tty: Arc<Tty>,
}

impl IrqHandler for SerialIrqHandler {
    fn raise(&mut self) -> crate::irq::Status {
        while unsafe { read8(COM1_BASE + LSR_REG) } & LSR_DATA_READY != 0 {
            let byte = unsafe { read8(COM1_BASE + DATA_REG) };
            self.tty.input_byte(byte);
        }

        crate::irq::Status::Handled
    }
}

#[initgraph::task(
    name = "arch.x86_64.serial",
    entails = [crate::arch::EARLY_INIT_STAGE],
)]
fn SERIAL_STAGE() {
    unsafe {
        write8(COM1_BASE + 1, 0x00); // Disable interrupts
        write8(COM1_BASE + 3, 0x80); // Enable DLAB (set baud rate divisor)#

        write8(COM1_BASE + 0, 0x03); // Set divisor low byte (115200 baud if 1)
        write8(COM1_BASE + 1, 0x00); // Set divisor high byte

        write8(COM1_BASE + 3, 0x03); // 8 bits, no parity, one stop bit (8n1)
        write8(COM1_BASE + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
        write8(COM1_BASE + 4, 0x0B); // IRQs enabled, RTS/DSR set

        write8(COM1_BASE + 7, 0xAE); // Send a test byte.
        // If we don't get the same value back, this serial port doesn't work.
        if read8(COM1_BASE + 7) != 0xAE {
            return;
        }

        write8(COM1_BASE + 4, 0x0F); // Disable loopback mode.
    };

    log::add_sink(Box::new(SerialLogger));
}

#[initgraph::task(
    name = "arch.x86_64.serial_file",
    depends = [
        crate::vfs::VFS_STAGE,
        crate::vfs::fs::devtmpfs::DEVTMPFS_STAGE,
        super::apic::IOAPIC_STAGE
    ],
)]
fn SERIAL_FILE_STAGE() {
    let tty = Tty::new(String::from("com1"), Arc::new(SerialTtyDriver));

    let irq = super::apic::get_isa_irq(4).unwrap() as Arc<dyn IrqLine>;
    irq.attach(Box::new(SerialIrqHandler { tty: tty.clone() }));
    unsafe { write8(COM1_BASE + 1, 0x01) };
    irq.unmask();

    tty.register_device().expect("Unable to create com1");
}
