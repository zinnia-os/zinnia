pub mod cache;
pub mod exec;
pub mod file;
pub mod fs;
pub mod inode;
pub mod pipe;

pub use cache::Entry;
pub use cache::PathNode;
pub use file::File;
pub use fs::Mount;
pub use fs::MountFlags;

use crate::{
    memory::{
        PagedMemoryObject, UserPtr, VirtAddr,
        virt::{AddressSpace, VmFlags},
    },
    posix::errno::{EResult, Errno},
    process::{Identity, PROCESS_STAGE, Process},
    uapi::{
        self,
        dirent::{DT_DIR, dirent},
        off_t,
    },
    util::once::Once,
    vfs::{
        cache::LookupFlags,
        file::{MmapFlags, OpenFlags},
        fs::devtmpfs,
        inode::{Device, Mode, NodeOps},
    },
};
use alloc::sync::Arc;
use core::num::NonZeroUsize;

/// The root directory entry.
static ROOT: Once<PathNode> = Once::new();

/// Gets a reference to the root of the VFS.
pub fn get_root() -> PathNode {
    ROOT.get().clone()
}

/// Creates a new directory.
pub fn mkdir(
    root: PathNode,
    cwd: PathNode,
    path: &[u8],
    mode: Mode,
    identity: &Identity,
) -> EResult<PathNode> {
    let path = PathNode::lookup(root.clone(), cwd, path, identity, LookupFlags::MustNotExist)?;

    let parent_inode = path
        .lookup_parent()?
        .entry
        .get_inode()
        .ok_or(Errno::ENOENT)?;
    parent_inode.try_access(identity, OpenFlags::Write, false)?;

    match &parent_inode.node_ops {
        NodeOps::Directory(x) => {
            x.mkdir(&parent_inode, path.clone(), mode, identity)?;
            Ok(path)
        }
        _ => Err(Errno::ENOTDIR),
    }
}

/// Creates a symbolic link at `path`, pointing to `target_path`.
pub fn symlink(
    root: PathNode,
    cwd: PathNode,
    path: &[u8],
    target_path: &[u8],
    identity: &Identity,
) -> EResult<()> {
    let path = PathNode::lookup(root.clone(), cwd, path, identity, LookupFlags::MustNotExist)?;

    let parent_inode = path
        .lookup_parent()?
        .entry
        .get_inode()
        .ok_or(Errno::ENOENT)?;
    parent_inode.try_access(identity, OpenFlags::Write, false)?;

    // Create the symlink in the parent directory.
    match &parent_inode.node_ops {
        NodeOps::Directory(x) => x.symlink(&parent_inode, path, target_path, identity),
        _ => Err(Errno::ENOTDIR),
    }
}

/// Creates a new node in the VFS.
pub fn mknod(
    root: PathNode,
    cwd: PathNode,
    path: &[u8],
    mode: Mode,
    device: Option<Device>,
    identity: &Identity,
) -> EResult<()> {
    let path = PathNode::lookup(root.clone(), cwd, path, identity, LookupFlags::MustNotExist)?;
    let parent = path
        .lookup_parent()
        .and_then(|p| p.entry.get_inode().ok_or(Errno::ENOENT))
        .expect("Entry has no parent node?");

    let dir = match &parent.node_ops {
        NodeOps::Directory(x) => x,
        _ => return Err(Errno::ENOTDIR),
    };

    let new_inode = dir.mknod(&parent, mode, device, identity)?;
    path.entry.set_inode(new_inode);

    Ok(())
}

/// Maps a memory object in the address space of a process.
pub fn mmap(
    file: Option<Arc<File>>,
    space: &mut AddressSpace,
    addr: VirtAddr,
    len: NonZeroUsize,
    prot: VmFlags,
    flags: MmapFlags,
    offset: uapi::off_t,
) -> EResult<VirtAddr> {
    if flags.contains(MmapFlags::Anonymous) {
        let anon = Arc::new(PagedMemoryObject::new_phys());
        space.map_object(anon, addr, len, prot, offset)?;
    } else if let Some(f) = file {
        f.ops.mmap(&f, space, addr, len, prot, flags, offset)?;
    } else {
        return Err(Errno::EINVAL);
    }

    return Ok(addr);
}

pub fn pipe() -> EResult<(Arc<File>, Arc<File>)> {
    let pipe = Arc::try_new(pipe::PipeBuffer::new())?;
    let endpoint1 = File::open_disconnected(pipe.clone(), OpenFlags::Read)?;
    let endpoint2 = File::open_disconnected(pipe, OpenFlags::Write)?;

    Ok((endpoint1, endpoint2))
}

pub fn get_dir_entries(
    file: Arc<File>,
    buffer: &mut [dirent],
    identity: &Identity,
) -> EResult<usize> {
    if buffer.is_empty() {
        return Ok(0);
    }

    let inode = file.inode.clone().ok_or(Errno::EBADF)?;
    let path = file.path.clone().unwrap();

    let mut offset = file.offset.lock();
    let mut num_read = 0;

    // Read `.` and `..` entries.
    if *offset == 0 {
        buffer[0] = dirent {
            d_ino: inode.id,
            d_off: 0,
            d_reclen: size_of::<dirent>() as _,
            d_type: DT_DIR,
            d_name: [0u8; _],
        };
        buffer[0].d_name[0] = b'.';
        num_read += 1;
    }

    if *offset == 1 || num_read == 1 {
        buffer[1] = dirent {
            d_ino: inode.id,
            d_off: 1,
            d_reclen: size_of::<dirent>() as _,
            d_type: DT_DIR,
            d_name: [0u8; _],
        };
        buffer[1].d_name[0] = b'.';
        buffer[1].d_name[1] = b'.';
        num_read += 1;
    }

    // Read normal dir entries.
    num_read += match &inode.node_ops {
        NodeOps::Directory(x) => x.get_dir_entries(
            &inode,
            path.entry,
            *offset as off_t,
            &mut buffer[num_read..],
            identity,
        )?,
        _ => return Err(Errno::ENOTDIR),
    };

    *offset += num_read as u64;

    for (i, entry) in buffer.iter_mut().enumerate() {
        entry.d_off = i as _;
    }

    Ok(num_read)
}

#[initgraph::task(
    name = "generic.vfs",
    depends = [crate::memory::MEMORY_STAGE],
)]
pub fn VFS_STAGE() {
    // Mount a tmpfs as root.
    let tmpfs = fs::mount(
        b"tmpfs",
        MountFlags::empty(),
        UserPtr::new(VirtAddr::null()),
    )
    .expect("Unable to mount the root tmpfs");

    let root_path = PathNode {
        entry: tmpfs.root.clone(),
        mount: tmpfs,
    };

    unsafe { ROOT.init(root_path.clone()) };
}

#[initgraph::task(
    name = "generic.vfs.dev-mount",
    depends = [VFS_STAGE, devtmpfs::DEVTMPFS_STAGE, PROCESS_STAGE],
)]
pub fn VFS_DEV_MOUNT_STAGE() {
    // Mount the devtmpfs on `/dev`.
    let devtmpfs = fs::mount(
        b"devtmpfs",
        MountFlags::empty(),
        UserPtr::new(VirtAddr::null()),
    )
    .expect("Unable to mount the devtmpfs");

    let proc = Process::get_kernel();
    let root = proc.root_dir.lock();
    let cwd = proc.working_dir.lock();
    mkdir(
        root.clone(),
        cwd.clone(),
        b"/dev",
        Mode::from_bits_truncate(0o755),
        Identity::get_kernel(),
    )
    .expect("Unable to create /dev")
    .mount(devtmpfs)
    .expect("Unable to mount devtmpfs on /dev");
}
