use crate::{
    posix::errno::EResult,
    process::Identity,
    util::once::Once,
    vfs::{
        self, Entry, Mount, MountFlags, PathNode,
        file::FileOps,
        fs::FileSystem,
        inode::{Mode, NodeType},
    },
};
use alloc::sync::Arc;

static DEV_MOUNT: Once<Arc<Mount>> = Once::new();

#[derive(Debug)]
struct DevTmpFs;

impl FileSystem for DevTmpFs {
    fn get_name(&self) -> &'static [u8] {
        b"devtmpfs"
    }

    fn mount(&self, _: Option<Arc<Entry>>, flags: MountFlags) -> EResult<Arc<Mount>> {
        let mount = DEV_MOUNT.get();
        Ok(Arc::new(Mount::new(flags, mount.root.clone())))
    }
}

#[initgraph::task(
    name = "generic.vfs.devtmpfs",
    depends = [super::tmpfs::TMPFS_INIT_STAGE],
    entails = [crate::vfs::VFS_STAGE],
)]
pub fn DEVTMPFS_STAGE() {
    super::register_fs(&DevTmpFs);

    // Ask for a singleton-like tmpfs.
    let tmpfs = super::mount(None, b"tmpfs", MountFlags::empty())
        .expect("Unable to create devtmpfs from tmpfs");

    unsafe { DEV_MOUNT.init(tmpfs) };
}

pub fn register_device(
    name: &[u8],
    device: Arc<dyn FileOps>,
    mode: Mode,
    is_block: bool,
) -> EResult<()> {
    let mount = DEV_MOUNT.get();

    let parent = PathNode {
        mount: mount.clone(),
        entry: mount.root.clone(),
    };

    vfs::mknod(
        parent.clone(),
        parent.clone(),
        name,
        if is_block {
            NodeType::BlockDevice
        } else {
            NodeType::CharacterDevice
        },
        mode,
        Some(device),
        Identity::get_kernel(),
    )
}
