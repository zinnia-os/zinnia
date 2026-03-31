use crate::{
    arch::x86_64::asm::{read8, write8},
    irq::{IrqHandler, IrqLine},
    log::{self, LoggerSink},
    memory::IovecIter,
    posix::errno::EResult,
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{File, file::FileOps, fs::devtmpfs, inode::Mode},
};
use alloc::{boxed::Box, sync::Arc};

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

struct SerialState {
    buffer: SpinMutex<RingBuffer>,
    rd_queue: Event,
}

struct SerialDevice {
    state: Arc<SerialState>,
}

struct SerialIrqHandler {
    state: Arc<SerialState>,
}

impl FileOps for SerialDevice {
    fn read(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let guard = self.state.rd_queue.guard();
        loop {
            {
                let mut inner = self.state.buffer.lock();
                if !inner.is_empty() {
                    let mut byte = [0u8; 1];
                    inner.read(&mut byte);
                    buffer.copy_from_slice(&byte)?;
                    return Ok(1);
                }
            }
            guard.wait();
        }
    }

    fn write(&self, _file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let mut buf = vec![0u8; buffer.len()];
        buffer.copy_to_slice(&mut buf)?;
        SerialLogger.write(&buf);
        Ok(buffer.len() as _)
    }
}

impl IrqHandler for SerialIrqHandler {
    fn raise(&mut self) -> crate::irq::Status {
        let mut any = false;

        // Drain all bytes currently available in the UART FIFO.
        while unsafe { read8(COM1_BASE + LSR_REG) } & LSR_DATA_READY != 0 {
            let byte = unsafe { read8(COM1_BASE + DATA_REG) };

            // Translate carriage return to newline (terminals send \r on Enter).
            let byte = if byte == b'\r' { b'\n' } else { byte };

            // Echo the character back so it appears on the console.
            while unsafe { read8(COM1_BASE + LSR_REG) } & LSR_THR_EMPTY == 0 {
                core::hint::spin_loop();
            }
            unsafe { write8(COM1_BASE + DATA_REG, byte) };
            if byte == b'\n' {
                while unsafe { read8(COM1_BASE + LSR_REG) } & LSR_THR_EMPTY == 0 {
                    core::hint::spin_loop();
                }
                //unsafe { write8(COM1_BASE + DATA_REG, b'\n') };
            }

            self.state.buffer.lock().write(&[byte]);
            any = true;
        }

        if any {
            self.state.rd_queue.wake_one();
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
    let state = Arc::new(SerialState {
        buffer: SpinMutex::new(RingBuffer::new(0x1000)),
        rd_queue: Event::new(),
    });

    let irq = super::apic::get_isa_irq(4).unwrap() as Arc<dyn IrqLine>;
    irq.unmask();
    irq.attach(Box::new(SerialIrqHandler {
        state: state.clone(),
    }));

    // Enable received data available interrupt.
    unsafe {
        write8(COM1_BASE + 1, 0x01);
    }

    devtmpfs::register_device(
        b"serial",
        Arc::new(SerialDevice { state }),
        Mode::from_bits_truncate(0o666),
        false,
    )
    .expect("Unable to create serial file");
}
