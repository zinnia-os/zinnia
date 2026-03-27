use super::fs::SuperBlock;
use crate::{
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi::{self, dirent::dirent, off_t, stat::*, time::timespec},
    util::mutex::spin::SpinMutex,
    vfs::{
        Entry, PathNode,
        file::{File, FileOps, OpenFlags},
    },
};
use alloc::sync::Arc;
use core::{any::Any, fmt::Debug};

/// A standalone file system node, also commonly referred to as a vnode.
/// It is used to represent a file or sized memory in a generic way.
pub struct INode {
    /// Operations that only work on a certain type of node.
    pub node_ops: NodeOps,
    /// The super block which this node is located in.
    pub sb: Option<Arc<dyn SuperBlock>>,

    // The following fields make up `stat`.
    pub id: usize,
    pub size: SpinMutex<usize>,
    pub uid: SpinMutex<uapi::uid_t>,
    pub gid: SpinMutex<uapi::gid_t>,
    pub atime: SpinMutex<timespec>,
    pub mtime: SpinMutex<timespec>,
    pub ctime: SpinMutex<timespec>,
    pub mode: SpinMutex<Mode>,
}

impl INode {
    pub fn new(ops: NodeOps, super_block: Option<Arc<dyn SuperBlock>>) -> Self {
        Self {
            node_ops: ops,
            sb: super_block,
            id: 0,
            size: SpinMutex::new(0),
            uid: SpinMutex::new(0),
            gid: SpinMutex::new(0),
            atime: SpinMutex::new(timespec::default()),
            mtime: SpinMutex::new(timespec::default()),
            ctime: SpinMutex::new(timespec::default()),
            mode: SpinMutex::new(Mode::empty()),
        }
    }

    /// Checks if the node can be accessed with the given identity.
    /// Returns [`Errno::EACCES`] if an access is not allowed.
    pub fn try_access(&self, ident: &Identity, flags: OpenFlags, use_real: bool) -> EResult<()> {
        let _ = use_real; // TODO
        let mode = self.mode.lock();

        if ident.effective_user_id == 0 {
            // If this file is not able to be executed, always fail.
            if flags.contains(OpenFlags::Executable)
                && !mode.contains(Mode::UserExec | Mode::GroupExec | Mode::OtherExec)
            {
                return Err(Errno::EACCES);
            }

            return Ok(());
        }

        todo!("Implement UID handling for !root");
    }

    pub fn len(&self) -> usize {
        *self.size.lock()
    }

    /// Updates the node with given timestamps.
    /// If an argument is [`None`], the respective value is not updated.
    pub fn update_time(
        &self,
        mtime: Option<timespec>,
        atime: Option<timespec>,
        ctime: Option<timespec>,
    ) {
        if let Some(mtime) = mtime {
            *self.mtime.lock() = mtime;
        }
        if let Some(atime) = atime {
            *self.atime.lock() = atime;
        }
        if let Some(ctime) = ctime {
            *self.ctime.lock() = ctime;
        }
    }

    /// Changes permissions on this `node`.
    pub fn chmod(&self, mode: Mode) {
        let mut m = self.mode.lock();
        *m = mode;
    }

    /// Changes ownership on this `node`.
    pub fn chown(&self, uid: uapi::uid_t, gid: uapi::gid_t) {
        *self.uid.lock() = uid;
        *self.gid.lock() = gid;
    }

    pub fn file_ops(&self) -> Arc<dyn FileOps> {
        match &self.node_ops {
            NodeOps::Regular(x) => x.clone(),
            NodeOps::Directory(x) => x.clone(),
            NodeOps::SymbolicLink(x) => x.clone(),
            NodeOps::FIFO(x) => x.clone(),
            NodeOps::BlockDevice(x) => x.clone(),
            NodeOps::CharacterDevice(x) => x.clone(),
            NodeOps::Socket(_) => todo!(),
        }
    }
}

/// Operations which work on any kind of [`INode`].
pub trait CommonOps: Debug {
    /// Synchronizes the node metadata back to the underlying file system.
    fn sync(&self, node: &INode) -> EResult<()>;
}

pub enum NodeOps {
    Regular(Arc<dyn RegularOps>),
    Directory(Arc<dyn DirectoryOps>),
    SymbolicLink(Arc<dyn SymlinkOps>),
    FIFO(Arc<dyn FileOps>),
    BlockDevice(Arc<dyn FileOps>),
    CharacterDevice(Arc<dyn FileOps>),
    Socket(Arc<()>), // TODO
}

/// Operations for directory [`INode`]s.
pub trait DirectoryOps: FileOps + Any {
    /// Looks up all children in an `node` directory and caches them in `entry`.
    /// An implementation shall return [`Errno::ENOENT`] if a lookup fails and
    /// shall leave `entry` unchanged.
    fn lookup(&self, self_node: &Arc<INode>, entry: &PathNode) -> EResult<()>;

    /// Opens a directory.
    fn open(
        &self,
        self_node: &Arc<INode>,
        entry: PathNode,
        flags: OpenFlags,
        identity: &Identity,
    ) -> EResult<Arc<File>>;

    /// Creates a new regular file. The implementation should create the [`INode`]
    /// and set it in the given `entry`.
    fn create(
        &self,
        self_node: &Arc<INode>,
        entry: Arc<Entry>,
        mode: Mode,
        identity: &Identity,
    ) -> EResult<()> {
        let _ = (self_node, entry, mode, identity);
        Err(Errno::EPERM)
    }

    /// Creates a new regular file. The implementation should create the [`INode`]
    /// and set it in the given `path.entry`.
    fn mkdir(
        &self,
        self_node: &Arc<INode>,
        path: PathNode,
        mode: Mode,
        identity: &Identity,
    ) -> EResult<()> {
        let _ = (self_node, path, mode, identity);
        Err(Errno::EPERM)
    }

    /// Creates a new symbolic link.
    fn symlink(
        &self,
        self_node: &Arc<INode>,
        path: PathNode,
        target_path: &[u8],
        identity: &Identity,
    ) -> EResult<()> {
        let _ = (self_node, path, target_path, identity);
        Err(Errno::EPERM)
    }

    /// Creates a new hard link.
    fn link(
        &self,
        self_node: &Arc<INode>,
        path: &PathNode,
        target: &Arc<INode>,
        identity: &Identity,
    ) -> EResult<()>;

    /// Removes a link.
    fn unlink(&self, self_node: &Arc<INode>, path: &PathNode, identity: &Identity) -> EResult<()>;

    /// Renames a node.
    fn rename(
        &self,
        self_node: &Arc<INode>,
        path: PathNode,
        target: &Arc<INode>,
        target_path: PathNode,
        identity: &Identity,
    ) -> EResult<()>;

    fn mknod(
        &self,
        self_node: &Arc<INode>,
        node_type: NodeType,
        mode: Mode,
        dev: Option<Arc<dyn FileOps>>,
        identity: &Identity,
    ) -> EResult<Arc<INode>> {
        let _ = (self_node, node_type, mode, dev, identity);
        Err(Errno::ENODEV)
    }

    /// Read directory entries into a buffer.
    /// Default implementation uses namecache.
    fn get_dir_entries(
        &self,
        self_node: &Arc<INode>,
        entry: Arc<Entry>,
        offset: off_t,
        buffer: &mut [dirent],
        identity: &Identity,
    ) -> EResult<usize> {
        let _ = (self_node, identity);
        let children = entry.children.lock();

        let mut read = 0;
        let mut current = 0;

        for (_, child) in children.iter() {
            if current < offset {
                current += 1;
                continue;
            }

            if read == buffer.len() {
                break;
            }

            let inode = match child.get_inode() {
                Some(x) => x,
                None => continue,
            };

            buffer[read] = dirent {
                d_ino: inode.id,
                d_off: (current) as _,
                d_reclen: size_of::<dirent>() as _,
                d_type: match inode.node_ops {
                    NodeOps::Regular(_) => uapi::dirent::DT_REG,
                    NodeOps::Directory(_) => uapi::dirent::DT_DIR,
                    NodeOps::SymbolicLink(_) => uapi::dirent::DT_LNK,
                    NodeOps::FIFO(_) => uapi::dirent::DT_FIFO,
                    NodeOps::BlockDevice(_) => uapi::dirent::DT_BLK,
                    NodeOps::CharacterDevice(_) => uapi::dirent::DT_CHR,
                    NodeOps::Socket(_) => uapi::dirent::DT_SOCK,
                },
                d_name: [0u8; _],
            };

            buffer[read].d_name[..child.name.len()].copy_from_slice(&child.name);

            read += 1;
            current += 1;
        }

        Ok(read)
    }
}

/// Operations for regular file [`INode`]s.
pub trait RegularOps: FileOps + Any {
    /// Truncates the node to a given new_length in bytes.
    /// `new_length` must be equal or less than the current node size.
    fn truncate(&self, node: &INode, new_length: u64) -> EResult<()>;
}

/// Operations for symbolic link [`INode`]s.
pub trait SymlinkOps: FileOps + Any {
    /// Reads the path of the symbolic link of the node.
    /// Returns amount of bytes read into the buffer.
    fn read_link(&self, node: &INode, buf: &mut [u8]) -> EResult<u64>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeType {
    Regular = S_IFREG as _,
    Directory = S_IFDIR as _,
    SymbolicLink = S_IFLNK as _,
    FIFO = S_IFIFO as _,
    BlockDevice = S_IFBLK as _,
    CharacterDevice = S_IFCHR as _,
    Socket = S_IFSOCK as _,
}

bitflags::bitflags! {
    #[derive(Debug, Default, Clone)]
    pub struct Mode: u32 {
        const UserRead = S_IRUSR;
        const UserWrite = S_IWUSR;
        const UserExec = S_IXUSR;

        const GroupRead = S_IRGRP;
        const GroupWrite = S_IWGRP;
        const GroupExec = S_IXGRP;

        const OtherRead = S_IROTH;
        const OtherWrite = S_IWOTH;
        const OtherExec = S_IXOTH;

        const SetUserId = S_ISUID;
        const SetGroupId = S_ISGID;

        const Sticky = S_ISVTX;
    }
}
