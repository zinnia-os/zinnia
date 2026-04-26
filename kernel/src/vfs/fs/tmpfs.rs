use super::{MountFlags, SuperBlock};
use crate::{
    arch,
    memory::{
        AddressSpace, IovecIter, PagedMemoryObject, UserPtr, VirtAddr, VmFlags, cache::MemoryObject,
    },
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi::{self, statvfs::statvfs},
    util::mutex::spin::SpinMutex,
    vfs::{
        PathNode,
        cache::{Entry, EntryState},
        file::{File, FileOps, MmapFlags, OpenFlags},
        fs::{FileSystem, Mount},
        inode::{Device, DirectoryOps, INode, Mode, NodeOps, RegularOps, SymlinkOps},
    },
};
use alloc::{sync::Arc, vec::Vec};
use core::{
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug)]
struct TmpFs;

impl FileSystem for TmpFs {
    fn get_name(&self) -> &'static [u8] {
        b"tmpfs"
    }

    fn mount(&self, flags: MountFlags, _: UserPtr<()>) -> EResult<Arc<Mount>> {
        let super_block = Arc::try_new(TmpSuper {
            inode_counter: AtomicUsize::new(0),
        })?;

        let dir = Arc::new(TmpDir);

        let root_entry = Arc::new(Entry::new(
            b"",
            Some(super_block.clone().create_inode(
                NodeOps::Directory(dir.clone()),
                Mode::from_bits_truncate(0o755),
            )?),
            None,
        ));

        Ok(Arc::new(Mount::new(flags, root_entry)))
    }
}

#[derive(Debug)]
struct TmpSuper {
    inode_counter: AtomicUsize,
}

impl TmpSuper {
    fn create_inode(self: Arc<Self>, node_ops: NodeOps, mode: Mode) -> EResult<Arc<INode>> {
        Ok(Arc::try_new(INode {
            id: self.inode_counter.fetch_add(1, Ordering::Acquire),
            node_ops,
            sb: Some(self),
            mode: SpinMutex::new(mode),
            atime: SpinMutex::default(),
            mtime: SpinMutex::default(),
            ctime: SpinMutex::default(),
            size: SpinMutex::default(),
            uid: SpinMutex::default(),
            gid: SpinMutex::default(),
        })?)
    }
}

impl SuperBlock for TmpSuper {
    fn sync(self: Arc<Self>) -> EResult<()> {
        // This is a no-op.
        Ok(())
    }

    fn statvfs(self: Arc<Self>) -> EResult<statvfs> {
        todo!()
    }
}

#[derive(Default)]
struct TmpDir;

impl DirectoryOps for TmpDir {
    fn lookup(&self, _: &Arc<INode>, _: &PathNode) -> EResult<()> {
        // tmpfs directories only live in memory, so we cannot look up entries that do not exist.
        return Err(Errno::ENOENT);
    }

    fn open(
        &self,
        node: &Arc<INode>,
        path: PathNode,
        flags: OpenFlags,
        _identity: &Identity,
    ) -> EResult<Arc<File>> {
        let file = File {
            path: Some(path),
            ops: node.file_ops(),
            inode: Some(node.clone()),
            flags: SpinMutex::new(flags),
            offset: SpinMutex::new(0),
            released: AtomicBool::new(false),
        };
        return Ok(Arc::try_new(file)?);
    }

    fn link(
        &self,
        _node: &Arc<INode>,
        path: &PathNode,
        target: &Arc<INode>,
        _identity: &Identity,
    ) -> EResult<()> {
        path.entry.set_inode(target.clone());
        Ok(())
    }

    fn unlink(&self, _node: &Arc<INode>, entry: &PathNode, _identity: &Identity) -> EResult<()> {
        *entry.entry.inode.lock() = EntryState::NotPresent;
        Ok(())
    }

    fn rename(
        &self,
        _node: &Arc<INode>,
        entry: PathNode,
        _target: &Arc<INode>,
        target_entry: PathNode,
        _identity: &Identity,
    ) -> EResult<()> {
        let inode = entry.entry.get_inode().ok_or(Errno::ENOENT)?;
        target_entry.entry.set_inode(inode);
        *entry.entry.inode.lock() = EntryState::NotPresent;

        // Transfer children for directory renames.
        let children = core::mem::take(&mut *entry.entry.children.lock());
        *target_entry.entry.children.lock() = children;

        Ok(())
    }

    fn symlink(
        &self,
        node: &Arc<INode>,
        path: PathNode,
        target_path: &[u8],
        identity: &Identity,
    ) -> EResult<()> {
        let _ = identity; // TODO
        let reg = Arc::new(TmpSymlink::default());

        let sb: Arc<TmpSuper> = Arc::downcast(node.sb.clone().unwrap()).unwrap();
        let sym_inode = sb.create_inode(
            NodeOps::SymbolicLink(reg.clone()),
            Mode::from_bits_truncate(0o777),
        )?;

        *reg.target.lock() = target_path.to_vec();
        *sym_inode.size.lock() = target_path.len();
        path.entry.set_inode(sym_inode.clone());
        Ok(())
    }

    fn create(
        &self,
        self_node: &Arc<INode>,
        entry: Arc<Entry>,
        mode: Mode,
        _identity: &Identity,
    ) -> EResult<()> {
        let new_file = Arc::new(TmpRegular::new());

        let sb: Arc<TmpSuper> = Arc::downcast(self_node.sb.clone().unwrap()).unwrap();
        let new_node = sb.create_inode(NodeOps::Regular(new_file), mode)?;
        entry.set_inode(new_node.clone());
        Ok(())
    }

    fn mkdir(
        &self,
        self_node: &Arc<INode>,
        path: PathNode,
        mode: Mode,
        _identity: &Identity,
    ) -> EResult<()> {
        let result_dir = Arc::new(TmpDir);
        let sb: Arc<TmpSuper> = Arc::downcast(self_node.sb.clone().unwrap()).unwrap();
        let result_inode = sb.create_inode(NodeOps::Directory(result_dir.clone()), mode)?;

        path.entry.set_inode(result_inode.clone());

        Ok(())
    }

    fn mknod(
        &self,
        self_node: &Arc<INode>,
        mode: Mode,
        dev: Option<Device>,
        _identity: &Identity,
    ) -> EResult<Arc<INode>> {
        let new_node = dev.ok_or(Errno::ENODEV)?;
        let sb: Arc<TmpSuper> = Arc::downcast(self_node.sb.clone().unwrap()).unwrap();
        sb.create_inode(
            match new_node {
                Device::BlockDevice(x) => NodeOps::BlockDevice(x),
                Device::CharacterDevice(x) => NodeOps::CharacterDevice(x),
                Device::Socket(x) => NodeOps::Socket(x),
            },
            mode,
        )
    }
}

#[derive(Debug, Default)]
struct TmpSymlink {
    pub target: SpinMutex<Vec<u8>>,
}

impl SymlinkOps for TmpSymlink {
    fn read_link(&self, _node: &INode, buf: &mut [u8]) -> EResult<u64> {
        let target = self.target.lock();
        let copy_size = buf.len().min(target.len());
        buf[0..copy_size].copy_from_slice(&target[0..copy_size]);
        Ok(copy_size as u64)
    }
}

impl FileOps for TmpSymlink {}
impl FileOps for TmpDir {}

#[derive(Debug)]
struct TmpRegular {
    /// A mappable page cache for the contents of the node.
    pub cache: Arc<PagedMemoryObject>,
}

impl TmpRegular {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(PagedMemoryObject::new_phys()),
        }
    }
}

impl RegularOps for TmpRegular {
    fn truncate(&self, node: &INode, length: u64) -> EResult<()> {
        *node.size.lock() = length as usize;
        Ok(())
    }
}

impl FileOps for TmpRegular {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
        let start = offset;

        if start as usize >= inode.len() {
            return Ok(0);
        }

        let copy_size = buffer.len().min(inode.len() - start as usize);
        let mut v = vec![0u8; copy_size];
        let actual = (self.cache.as_ref() as &dyn MemoryObject).read(&mut v, start as usize);
        buffer.copy_from_slice(&v)?;

        Ok(actual as _)
    }

    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
        let mut size_lock = inode.size.lock();
        let start = offset;

        let mut v = vec![0u8; buffer.len()];
        buffer.copy_to_slice(&mut v)?;
        let actual = (self.cache.as_ref() as &dyn MemoryObject).write(&v, start as usize);
        *size_lock = (*size_lock).max(start as usize + actual);

        Ok(actual as _)
    }

    fn mmap(
        &self,
        _file: &File,
        space: &mut AddressSpace,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        flags: MmapFlags,
        offset: uapi::off_t,
    ) -> EResult<VirtAddr> {
        let object = if flags.contains(MmapFlags::Private) {
            self.cache.make_private(len, offset as usize)?
        } else {
            self.cache.clone()
        };

        let page_size = arch::virt::get_page_size();
        let misalign = addr.value() & (page_size - 1);
        let map_address = addr - misalign;
        let backed_map_size = (len.get() + misalign + page_size - 1) & !(page_size - 1);

        space.map_object(
            object,
            map_address,
            NonZeroUsize::new(backed_map_size).unwrap(),
            prot,
            offset - misalign as isize,
        )?;
        Ok(addr)
    }
}

#[initgraph::task(
    name = "generic.vfs.tmpfs",
    depends = [crate::memory::MEMORY_STAGE],
    entails = [crate::vfs::VFS_STAGE],
)]
pub fn TMPFS_INIT_STAGE() {
    super::register(&TmpFs);
}
