use super::internal;
use crate::{percpu::CpuData, posix::errno::EResult};

pub fn setup_bsp() {
    internal::cpu::setup_bsp()
}

/// Returns the value of the frame pointer register.
pub fn get_frame_pointer() -> usize {
    internal::cpu::get_frame_pointer()
}

/// Returns the per-CPU data of this CPU.
pub fn get_per_cpu() -> *mut CpuData {
    internal::cpu::get_per_cpu()
}

/// Performs some CPU-dependent operation.
pub fn archctl(cmd: usize, arg: usize) -> EResult<usize> {
    internal::cpu::archctl(cmd, arg)
}

/// Stop all other CPUs.
pub fn halt_others() {
    internal::cpu::halt_others()
}

/// Stop this CPU.
pub fn halt() -> ! {
    internal::cpu::halt()
}
