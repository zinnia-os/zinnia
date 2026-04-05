use crate::{
    boot::BootInfo,
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
pub struct CmdlineFile;

impl FileOps for CmdlineFile {
    fn read(&self, _: &File, iter: &mut IovecIter, offset: u64) -> EResult<isize> {
        let bytes = BootInfo::get().command_line.inner().as_bytes();
        let offset = (offset as usize).min(bytes.len());
        let bytes = bytes.get(offset..).ok_or(Errno::ERANGE)?;
        iter.copy_from_slice(bytes)
    }
}

#[initgraph::task(
    name = "generic.device.memfiles",
    depends = [PROCESS_STAGE, DEVTMPFS_STAGE]
)]
fn CMDLINE_STAGE() {
    devtmpfs::register_device(
        b"cmdline",
        Device::CharacterDevice(Arc::new(CmdlineFile)),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/cmdline");
}
