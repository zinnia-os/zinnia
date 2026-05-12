#[cfg(all(
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ),
    feature = "acpi"
))]
pub mod acpi;
pub mod block;
pub mod cmdline;
pub mod drm;
pub mod dt;
pub mod fbcon;
pub mod input;
pub mod kmsg;
pub mod memfiles;
pub mod net;
pub mod pci;
pub mod tty;
pub mod vt;

use crate::{
    posix::errno::EResult,
    vfs::file::{FileOps, OpenFlags},
};
use alloc::sync::Arc;
use core::any::Any;

pub trait Device: Sync + Send + Any {
    fn open(self: Arc<Self>, flags: OpenFlags) -> EResult<Arc<dyn FileOps>>;
    fn major(&self) -> u32;
    fn minor(&self) -> u32;
}

struct SharedDevice {
    ops: Arc<dyn FileOps>,
    major: u32,
    minor: u32,
}

impl Device for SharedDevice {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(self.ops.clone())
    }

    fn major(&self) -> u32 {
        self.major
    }

    fn minor(&self) -> u32 {
        self.minor
    }
}

/// Creates a fake device that wraps around FileOps.
pub fn make_shared(ops: Arc<dyn FileOps>, major: u32, minor: u32) -> Arc<dyn Device> {
    Arc::new(SharedDevice { ops, major, minor })
}
