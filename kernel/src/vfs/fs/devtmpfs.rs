use crate::{
    memory::{UserPtr, VirtAddr},
    posix::errno::EResult,
    util::once::Once,
    vfs::{Mount, MountFlags, PathNode, fs::FileSystem},
};
use alloc::sync::Arc;

static DEV_MOUNT: Once<Arc<Mount>> = Once::new();

#[derive(Debug)]
struct DevTmpFs;

impl FileSystem for DevTmpFs {
    fn get_name(&self) -> &'static [u8] {
        b"devtmpfs"
    }

    fn mount(&self, flags: MountFlags, _: UserPtr<()>) -> EResult<Arc<Mount>> {
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
    super::register(&DevTmpFs);

    // Ask for a singleton-like tmpfs.
    let tmpfs = super::mount(
        b"tmpfs",
        MountFlags::empty(),
        UserPtr::new(VirtAddr::null()),
    )
    .expect("Unable to create devtmpfs from tmpfs");

    unsafe { DEV_MOUNT.init(tmpfs) };
}

/// Returns the root [`PathNode`] for the devtmpfs mount (`/dev`).
pub fn get_root() -> PathNode {
    let mount = DEV_MOUNT.get();
    PathNode {
        mount: mount.clone(),
        entry: mount.root.clone(),
    }
}
