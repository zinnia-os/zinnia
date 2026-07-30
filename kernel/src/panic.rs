//! The global panic handler. Panics are usually non-recoverable error cases.
//! It halts all kernel function and prints some info about the machine state.

use super::log::GLOBAL_LOGGERS;
use crate::{
    arch,
    {
        memory::{VirtAddr, virt::mmu::PageTable},
        percpu::CpuData,
        vfs::exec::elf::ElfAddr,
    },
};
use core::panic::PanicInfo;

#[repr(C)]
#[derive(Debug)]
struct StackFrame {
    prev: *const StackFrame,
    return_addr: *const (),
}

macro_rules! log_panic {
    ($($arg:tt)*) => ({
        use core::fmt::Write;
        #[allow(unused)]
        let writer = unsafe { GLOBAL_LOGGERS.raw_inner().as_mut().unwrap() };
        _ = writer.write_fmt(format_args!("[    !!!!    ] \x1b[31m"));
        _ = writer.write_fmt(format_args!($($arg)*));
        _ = writer.write_fmt(format_args!("\x1b[0m\n"));
    });
}

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    unsafe { arch::irq::set_irq_state(false) };
    arch::cpu::halt_others();

    // We write directly to the loggers because something might've happened to the timers.
    log_panic!(
        "Kernel panic on CPU {}: {}",
        CpuData::get().id,
        info.message()
    );
    if let Some(location) = info.location() {
        log_panic!("at {}", location);
    }

    {
        log_panic!("----------");
        let modules = unsafe { super::module::MODULE_TABLE.raw_inner().as_mut().unwrap() };
        log_panic!("{} linked module(s):", modules.len());
        for (name, module) in modules.iter() {
            log_panic!(
                "{:?}: {}",
                module
                    .mappings
                    .first()
                    .map(|x| x.1)
                    .unwrap_or(VirtAddr::null()),
                name
            );
        }
    }

    // Do a stack trace.
    unsafe {
        let table = super::module::SYMBOL_TABLE.raw_inner().as_mut().unwrap();

        log_panic!("----------");
        log_panic!("Stack trace (most recent call first):");

        let mut fp = arch::cpu::get_frame_pointer() as *const StackFrame;
        let kernel_map = PageTable::get_kernel();

        /// Max stack trace depth to iterate.
        const MAX_STACK_FRAMES: usize = 32;

        let mut frames = 0;
        while frames < MAX_STACK_FRAMES && kernel_map.is_mapped(VirtAddr::from(fp)) {
            if !(fp as usize).is_multiple_of(align_of::<StackFrame>()) {
                break;
            }

            let addr = (*fp).return_addr as ElfAddr;
            if addr == 0 {
                break;
            }

            let symbol = table.iter().find(|(_, (sym, _))| {
                (addr >= sym.st_value) && (addr <= (sym.st_value + sym.st_size))
            });
            let (name, offset) = symbol
                .map(|(name, (sym, _))| (name.as_str(), addr - sym.st_value))
                .unwrap_or(("???", 0));

            log_panic!(
                "{:#x} [{:#} + {:#x}]",
                addr as u64,
                rustc_demangle::demangle(name),
                offset
            );

            let prev = (*fp).prev;
            if prev <= fp {
                break;
            }
            fp = prev;
            frames += 1;
        }

        if frames == MAX_STACK_FRAMES {
            log_panic!("... trace truncated at {MAX_STACK_FRAMES} frames");
        }
    }

    log_panic!("----------");
    log_panic!("End of panic message");

    arch::cpu::halt();
}
