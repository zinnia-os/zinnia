//! Message logs from the kernel.
// TODO: Convert to struct Console

use super::util::mutex::spin::SpinMutex;
use alloc::{boxed::Box, vec::Vec};
use core::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::task;
use crate::{memory::IovecIter, posix::errno::EResult};

/// A sink to write logs to.
pub trait LoggerSink: Send {
    fn name(&self) -> &'static str;

    /// Writes a buffer to the logger.
    fn write(&mut self, input: &[u8]);
}

const MAX_LOGGERS: usize = 16;

pub struct Logger {
    pub sinks: [Option<Box<dyn LoggerSink>>; MAX_LOGGERS],
}

/// Adds a sink to the logger.
pub fn add_sink(mut sink: Box<dyn LoggerSink>) {
    // Clear the output of the logger.
    sink.write(b"\x1bc");

    let name = sink.name();
    {
        let mut logger = GLOBAL_LOGGERS.lock();
        for s in &mut logger.sinks {
            match s {
                Some(_) => continue,
                None => {
                    *s = Some(sink);
                    break;
                }
            }
        }
    }
    log!("Registered new logging sink \"{}\"", name);
}

/// Removes a named sink from the logger.
pub fn remove_sink(name: &str) {
    let mut logger = GLOBAL_LOGGERS.lock();
    for sink in &mut logger.sinks {
        if let Some(x) = sink
            && x.name() == name
        {
            *sink = None;
        }
    }
}

/// The global registry for loggers.
pub static GLOBAL_LOGGERS: SpinMutex<Logger> = SpinMutex::new(Logger {
    sinks: [const { None }; MAX_LOGGERS],
});

impl fmt::Write for Logger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for x in self.sinks.iter_mut().flatten() {
            x.write(s.as_bytes());
        }
        Ok(())
    }
}

/// A static buffer to write early messages to.
static EARLY_BUFFER: SpinMutex<[u8; EARLY_BUFFER_LEN]> = SpinMutex::new([0; EARLY_BUFFER_LEN]);
/// The size of the early kernel buffer.
const EARLY_BUFFER_LEN: usize = 8192;
/// The current offset in this logger
static EARLY_BUFFER_ADDR: AtomicUsize = AtomicUsize::new(0);

/// Global in-memory logger.
static KERNEL_LOGGER: SpinMutex<Vec<u8>> = SpinMutex::new(Vec::new());

/// Primitive Logger sink for the kernel.
pub struct KernelLogger;
impl LoggerSink for KernelLogger {
    fn name(&self) -> &'static str {
        "kernel"
    }

    fn write(&mut self, input: &[u8]) {
        // As long as the message fits in the early buffer, use that.
        let offset = EARLY_BUFFER_ADDR.load(Ordering::Acquire);

        if offset + input.len() < EARLY_BUFFER_LEN {
            EARLY_BUFFER.lock()[offset..][..input.len()].copy_from_slice(input);
            EARLY_BUFFER_ADDR.fetch_add(input.len(), Ordering::Release);
        } else {
            let mut logger = KERNEL_LOGGER.lock();
            logger.extend_from_slice(input);
        }
    }
}

task!(attr(name = "Initialize kernel logger") {
    pub fn init() {
        let early_len = EARLY_BUFFER_ADDR.load(Ordering::Acquire);
        {
            let mut logger = KERNEL_LOGGER.lock();
            logger.extend_from_slice(&EARLY_BUFFER.lock()[..early_len]);
        }

        // Force all subsequent writes through the allocated logger. The early
        // buffer has been copied above and must not be returned a second time.
        EARLY_BUFFER_ADDR.store(EARLY_BUFFER_LEN, Ordering::Release);
        add_sink(Box::new(KernelLogger));
    }
});

pub fn read_kernel_log(target: &mut IovecIter, offset: u64) -> EResult<isize> {
    let logger = KERNEL_LOGGER.lock();
    let offset = (offset as usize).min(logger.len());
    target.copy_from_slice(&logger[offset..])
}
