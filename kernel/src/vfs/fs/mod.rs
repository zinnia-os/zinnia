pub mod devtmpfs;
pub mod initramfs;
mod tmpfs;

use super::inode::INode;
use crate::{
    posix::errno::{EResult, Errno},
    uapi::{mount::*, statvfs::*},
    util::mutex::spin::SpinMutex,
    vfs::{
        PathNode,
        cache::Entry,
        inode::{Mode, NodeOps},
    },
};
use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc};
use core::fmt::Debug;

/// A mounted file system.
#[derive(Debug)]
pub struct Mount {
    pub flags: MountFlags,
    pub root: Arc<Entry>,
    pub mount_point: SpinMutex<Option<PathNode>>,
}

impl Mount {
    pub fn new(flags: MountFlags, root: Arc<Entry>) -> Mount {
        Self {
            flags,
            root,
            mount_point: SpinMutex::new(None),
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone)]
    pub struct MountFlags: u32 {
        const ReadOnly = MNT_RDONLY;
        const NoSetUid = MNT_NOSUID;
        const NoExec = MNT_NOEXEC;
        const RelativeTime = MNT_RELATIME;
        const NoAccessTime = MNT_NOATIME;
        const Remount = MNT_REMOUNT;
        const Force = MNT_FORCE;
    }
}

pub trait FileSystem: Sync + Send + Debug {
    /// Returns an identifier which can be used to determine this file system.
    fn get_name(&self) -> &'static [u8];

    /// Mounts an instance of this file system from a `source`.
    /// Returns a reference to the mount point with an instance of this file system.
    /// Some file systems don't require a `source` and may ignore the argument.
    fn mount(&self, source: Option<Arc<Entry>>, flags: MountFlags) -> EResult<Arc<Mount>>;
}

/// A super block is the control structure of a file system instance.
/// It manages inodes.
pub trait SuperBlock: Sync + Send {
    /// Synchronizes the entire file system.
    fn sync(self: Arc<Self>) -> EResult<()>;

    /// Gets the status of the file system.
    fn statvfs(self: Arc<Self>) -> EResult<statvfs>;

    /// Allocates a new inode on this super block.
    /// If `node_type` is a character or block device, a `device` must also be passed.
    fn create_inode(self: Arc<Self>, node_ops: NodeOps, mode: Mode) -> EResult<Arc<INode>>;

    /// Deletes the inode.
    fn destroy_inode(self: Arc<Self>, inode: INode) -> EResult<()>;
}

/// A map of all known and registered file systems.
static FS_TABLE: SpinMutex<BTreeMap<&'static [u8], &'static dyn FileSystem>> =
    SpinMutex::new(BTreeMap::new());

/// Registers a new file system.
pub fn register_fs(fs: &'static dyn FileSystem) {
    let name = fs.get_name();
    FS_TABLE.lock().insert(name, fs);
    log!(
        "Registered new file system \"{}\"",
        String::from_utf8_lossy(name)
    );
}

/// Mounts a file system at path `source` on `target`.
pub fn mount(source: Option<Arc<Entry>>, fs_name: &[u8], flags: MountFlags) -> EResult<Arc<Mount>> {
    let table = FS_TABLE.lock();
    let fs = table.get(fs_name).ok_or(Errno::ENODEV)?;

    let mount = fs.mount(source, flags)?;

    Ok(mount)
}
