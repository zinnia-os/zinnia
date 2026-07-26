use super::SuperBlock;
use crate::{
    clock,
    posix::errno::EResult,
    process::Identity,
    uapi::{statvfs::statvfs, time::timespec},
    util::{
        mutex::{Mutex, spin::SpinMutex},
        once::Once,
    },
    vfs::inode::{INode, Mode, NodeOps},
};
use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

/// An internal pseudo file system which backs files that are not reachable through any path,
/// like pipes, sockets and anonymous fds. It is never mounted and only hands out inodes.
#[derive(Debug)]
struct AnonSuper {
    inode_counter: AtomicUsize,
}

impl SuperBlock for AnonSuper {
    fn sync(self: Arc<Self>) -> EResult<()> {
        // This is a no-op.
        Ok(())
    }

    fn statvfs(self: Arc<Self>) -> EResult<statvfs> {
        Ok(statvfs {
            f_bsize: 0,
            f_frsize: 0,
            f_blocks: 0,
            f_bfree: 0,
            f_bavail: 0,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_fsid: 0,
            f_flag: 0,
            f_namemax: 0,
            f_basetype: [0; 80],
        })
    }
}

static ANON_SUPER: Once<Arc<AnonSuper>> = Once::new();

pub fn create_anon_inode(
    node_ops: NodeOps,
    mode: Mode,
    identity: &Identity,
) -> EResult<Arc<INode>> {
    let sb = ANON_SUPER.get();
    let now = timespec::from_duration(clock::realtime().unwrap_or(Duration::ZERO));
    Ok(Arc::try_new(INode {
        id: sb.inode_counter.fetch_add(1, Ordering::Acquire),
        node_ops,
        sb: Some(sb.clone()),
        mode: SpinMutex::new(mode),
        atime: SpinMutex::new(now),
        mtime: SpinMutex::new(now),
        ctime: SpinMutex::new(now),
        size: SpinMutex::default(),
        uid: SpinMutex::new(identity.effective_user_id),
        gid: SpinMutex::new(identity.effective_group_id),
        append_lock: Mutex::new(()),
    })?)
}

#[task(
    name = "generic.vfs.anonfs",
    depends = [crate::memory::MEMORY_STAGE],
    entails = [crate::vfs::VFS_STAGE],
)]
pub fn ANONFS_STAGE() {
    // Start inode ids at 1 so anonymous files never expose st_ino == 0.
    unsafe {
        ANON_SUPER.init(Arc::new(AnonSuper {
            inode_counter: AtomicUsize::new(1),
        }))
    };
}
