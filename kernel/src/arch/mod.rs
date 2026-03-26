//! This module and all submodules contain architecture dependent code.
//! All architectures should implement all functions referenced by the submodules of this module.
//!
//! # STOP!
//! If you were looking to use functions from this module,
//! please reconsider and check if there isn't a wrapper around these functions!

#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(target_arch = "x86_64")]
mod internal {
    pub use super::x86_64::*;
}

#[cfg(target_arch = "riscv64")]
mod riscv64;

#[cfg(target_arch = "riscv64")]
mod internal {
    pub use super::riscv64::*;
}

pub mod cpu;
pub mod irq;
pub mod sched;
pub mod virt;

#[initgraph::task(name = "arch.early-init")]
pub fn EARLY_INIT_STAGE() {}

#[initgraph::task(
    name = "arch.init",
    depends = [EARLY_INIT_STAGE],
)]
pub fn INIT_STAGE() {}
