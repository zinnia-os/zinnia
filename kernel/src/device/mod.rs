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
pub mod devctl;
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
    device::block::BlockDevice,
    posix::errno::EResult,
    process::Identity,
    vfs::{
        self,
        file::{FileOps, OpenFlags},
        fs::devtmpfs,
        inode::{MknodTarget, Mode},
    },
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

fn dev_relative(name: &[u8]) -> &[u8] {
    name.strip_prefix(b"/").unwrap_or(name)
}

pub fn register_char_node(name: &[u8], device: Arc<dyn Device>, mode: Mode) -> EResult<()> {
    let root = devtmpfs::get_root();
    vfs::mknod(
        root.clone(),
        root,
        name,
        mode,
        Some(MknodTarget::CharacterDevice(device)),
        Identity::get_kernel(),
    )?;
    devctl::notify_create(dev_relative(name));
    Ok(())
}

/// Creates a `/dev` block node and announces it via [`devctl`].
pub fn register_block_node(name: &[u8], device: Arc<dyn BlockDevice>, mode: Mode) -> EResult<()> {
    let root = devtmpfs::get_root();
    vfs::mknod(
        root.clone(),
        root,
        name,
        mode,
        Some(MknodTarget::BlockDevice(device)),
        Identity::get_kernel(),
    )?;
    devctl::notify_create(dev_relative(name));
    Ok(())
}

/// Removes a `/dev` node and announces its departure via [`devctl`].
pub fn unregister_node(name: &[u8]) -> EResult<()> {
    let root = devtmpfs::get_root();
    vfs::unlink(root.clone(), root, name, Identity::get_kernel())?;
    devctl::notify_destroy(dev_relative(name));
    Ok(())
}
