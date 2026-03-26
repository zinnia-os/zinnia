use crate::util::once::Once;
use ::core::sync::atomic::AtomicBool;

mod asm;
mod consts;
pub mod cpu;
pub mod irq;
pub mod sched;
pub mod system;
pub mod virt;

pub struct ArchPerCpu {
    /// Size of the FPU.
    pub fpu_size: Once<usize>,
    /// Function called to save the FPU context.
    pub fpu_save: Once<unsafe fn(*mut u8)>,
    /// Function called to restore the FPU context.
    pub fpu_restore: Once<unsafe fn(*const u8)>,
    /// If this CPU supports the STAC/CLAC instructions.
    pub _can_smap: AtomicBool,
}

per_cpu!(
    pub(crate) static ARCH_DATA: ArchPerCpu = ArchPerCpu {
        fpu_size: Once::new(),
        fpu_save: Once::new(),
        fpu_restore: Once::new(),
        _can_smap: AtomicBool::new(false),
    };
);
