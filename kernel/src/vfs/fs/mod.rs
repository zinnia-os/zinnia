pub mod devtmpfs;
pub mod initramfs;
mod tmpfs;

use crate::{
    memory::UserPtr,
    posix::errno::{EResult, Errno},
    uapi::{mount::*, statvfs::*},
    util::mutex::spin::SpinMutex,
    vfs::{PathNode, cache::Entry},
};
use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::{any::Any, fmt::Debug};

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

pub trait FileSystem: Sync + Send {
    /// Returns an identifier which can be used to determine this file system.
    fn get_name(&self) -> &[u8];

    /// Mounts an instance of this file system from a `source`.
    /// Returns a reference to the mount point with an instance of this file system.
    /// Some file systems don't require a `source` and may ignore the argument.
    fn mount(&self, flags: MountFlags, arg: UserPtr<()>) -> EResult<Arc<Mount>>;
}

/// A super block is the control structure of a file system instance.
/// It manages inodes.
pub trait SuperBlock: Sync + Send + Any {
    /// Synchronizes the entire file system.
    fn sync(self: Arc<Self>) -> EResult<()>;

    /// Gets the status of the file system.
    fn statvfs(self: Arc<Self>) -> EResult<statvfs>;
}

/// A map of all known and registered file systems.
static FS_TABLE: SpinMutex<BTreeMap<&'static [u8], &'static dyn FileSystem>> =
    SpinMutex::new(BTreeMap::new());

/// Registers a new file system.
pub fn register(fs: &'static dyn FileSystem) {
    let name = fs.get_name();
    FS_TABLE.lock().insert(name, fs);
    log!(
        "Registered new file system \"{}\"",
        String::from_utf8_lossy(name)
    );
}

static MOUNTED_SUPERS: SpinMutex<Vec<Arc<dyn SuperBlock>>> = SpinMutex::new(Vec::new());

pub fn sync_all() -> EResult<()> {
    let supers = MOUNTED_SUPERS.lock().clone();
    let mut result = Ok(());
    for sb in supers {
        if let Err(e) = sb.sync() {
            result = Err(e);
        }
    }
    result
}

/// Mounts a file system at path `source` on `target`.
pub fn mount(fs_name: &[u8], flags: MountFlags, arg: UserPtr<()>) -> EResult<Arc<Mount>> {
    let fs = {
        let table = FS_TABLE.lock();
        *table.get(fs_name).ok_or(Errno::ENODEV)?
    };

    let mount = fs.mount(flags, arg)?;

    if let Some(sb) = mount.root.get_inode().and_then(|inode| inode.sb.clone()) {
        let mut supers = MOUNTED_SUPERS.lock();
        if !supers.iter().any(|s| Arc::ptr_eq(s, &sb)) {
            supers.push(sb);
        }
    }

    Ok(mount)
}
