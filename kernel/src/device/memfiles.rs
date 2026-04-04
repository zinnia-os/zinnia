use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    process::PROCESS_STAGE,
    vfs::{
        File,
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
    devtmpfs::register_device(
        b"null",
        Device::CharacterDevice(Arc::new(NullFile)),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/null");

    devtmpfs::register_device(
        b"full",
        Device::CharacterDevice(Arc::new(FullFile)),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/full");

    devtmpfs::register_device(
        b"zero",
        Device::CharacterDevice(Arc::new(ZeroFile)),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/zero");
}
