use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    process::{Identity, PROCESS_STAGE},
    vfs::{
        self, File,
        file::FileOps,
        fs::devtmpfs::{self, DEVTMPFS_STAGE},
        inode::{Device, Mode},
    },
};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct NullFile;

impl FileOps for NullFile {
    fn read(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        Ok(0)
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        Ok(buffer.len() as _)
    }
}

#[derive(Debug)]
pub struct ZeroFile;

impl FileOps for ZeroFile {
    fn read(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        buffer.fill(0)?;
        Ok(buffer.len() as _)
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        Ok(buffer.len() as _)
    }
}

#[derive(Debug)]
pub struct FullFile;

impl FileOps for FullFile {
    fn read(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        buffer.fill(0)?;
        Ok(buffer.len() as _)
    }

    fn write(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        Err(Errno::ENOSPC)
    }
}

#[initgraph::task(
    name = "generic.device.memfiles",
    depends = [PROCESS_STAGE, DEVTMPFS_STAGE]
)]
fn MEMFILES_STAGE() {
    let root = devtmpfs::get_root();

    vfs::mknod(
        root.clone(),
        root.clone(),
        b"null",
        Mode::from_bits_truncate(0o666),
        Some(Device::CharacterDevice(Arc::new(NullFile))),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/null");

    vfs::mknod(
        root.clone(),
        root.clone(),
        b"full",
        Mode::from_bits_truncate(0o666),
        Some(Device::CharacterDevice(Arc::new(FullFile))),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/full");

    vfs::mknod(
        root.clone(),
        root,
        b"zero",
        Mode::from_bits_truncate(0o666),
        Some(Device::CharacterDevice(Arc::new(ZeroFile))),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/zero");
}
