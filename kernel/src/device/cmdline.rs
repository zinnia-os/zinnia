use crate::{
    boot::BootInfo,
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
    let root = devtmpfs::get_root();

    vfs::mknod(
        root.clone(),
        root.clone(),
        b"cmdline",
        Mode::from_bits_truncate(0o666),
        Some(Device::CharacterDevice(Arc::new(CmdlineFile))),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/cmdline");
}
