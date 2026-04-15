use super::inode::INode;
use crate::{
    memory::{AddressSpace, IovecIter, VirtAddr, VmFlags},
    posix::errno::{EResult, Errno},
    process::Identity,
    uapi,
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::{
        cache::{LookupFlags, PathNode},
        inode::{Mode, NodeOps},
    },
};
use alloc::sync::Arc;
use core::{
    any::Any,
    fmt::Debug,
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
};
use uapi::{fcntl::*, mman::*};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct OpenFlags: u32 {
        /// Create the file if it's missing.
        const Create = O_CREAT;
        /// Exclusive use.
        const Exclusive = O_EXCL;
        /// Do not assign a controlling terminal.
        const NoCtrlTerminal = O_NOCTTY;
        const Truncate = O_TRUNC;
        const Append = O_APPEND;
        const NonBlocking = O_NONBLOCK;
        const SyncData = O_DSYNC;
        /// Open this file as a directory.
        const Directory = O_DIRECTORY;
        /// Don't follow symbolic links.
        const NoFollow = O_NOFOLLOW;
        /// Close this file on a call to `execve`.
        const CloseOnExec = O_CLOEXEC;
        const Sync = O_SYNC;
        const SyncRead = O_RSYNC;
        const LargeFile = O_LARGEFILE;
        /// Don't update the access time.
        const NoAccessTime = O_NOATIME;
        const Temporary = O_TMPFILE;
        const Read = O_RDONLY;
        const Write = O_WRONLY;
        const ReadWrite = O_RDWR;
        const Executable = O_EXEC;
    }

    #[derive(Debug, Clone, Copy)]
    pub struct MmapFlags: u32 {
        const Anonymous = MAP_ANONYMOUS;
        const Shared = MAP_SHARED;
        const Private = MAP_PRIVATE;
        const Fixed = MAP_FIXED;
    }
}

#[derive(Debug)]
pub enum SeekAnchor {
    /// Seek relative to the start of the file.
    Start(u64),
    /// Seek relative to the current cursor position.
    Current(i64),
    /// Seek relative to the end of the file.
    End(i64),
}

/// The kernel representation of an open file.
pub struct File {
    /// The cached entry for this file.
    pub path: Option<PathNode>,
    /// Operations that can be performed on this file.
    pub ops: Arc<dyn FileOps>,
    /// The opened inode.
    pub inode: Option<Arc<INode>>,
    /// File open flags.
    pub flags: SpinMutex<OpenFlags>,
    /// Byte offset into the file.
    pub offset: SpinMutex<u64>,
}

pub struct FileDescription {
    pub file: Arc<File>,
    pub close_on_exec: AtomicBool,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PollFlags: i16 {
        const In = uapi::poll::POLLIN;
        const Out = uapi::poll::POLLOUT;
        const Pri = uapi::poll::POLLPRI;
        const Hup = uapi::poll::POLLHUP;
        const Err = uapi::poll::POLLERR;
        const Rdhup = uapi::poll::POLLRDHUP;
        const Nval = uapi::poll::POLLNVAL;
        const Rdnorm = uapi::poll::POLLRDNORM;
        const Rdband = uapi::poll::POLLRDBAND;
        const Wrnorm = uapi::poll::POLLWRNORM;
        const Wrband = uapi::poll::POLLWRBAND;

        const Read = uapi::poll::POLLIN | uapi::poll::POLLPRI | uapi::poll::POLLRDHUP | uapi::poll::POLLRDNORM | uapi::poll::POLLRDBAND;
        const Write = uapi::poll::POLLOUT | uapi::poll::POLLWRNORM | uapi::poll::POLLWRBAND;
    }
}

#[derive(Clone, Copy, Default)]
pub struct PollEventSet<'a> {
    first: Option<&'a Event>,
    second: Option<&'a Event>,
}

impl<'a> PollEventSet<'a> {
    pub const fn new() -> Self {
        Self {
            first: None,
            second: None,
        }
    }

    pub const fn one(event: &'a Event) -> Self {
        Self {
            first: Some(event),
            second: None,
        }
    }

    pub fn add(mut self, event: &'a Event) -> Self {
        if self
            .first
            .is_some_and(|existing| core::ptr::eq(existing, event))
            || self
                .second
                .is_some_and(|existing| core::ptr::eq(existing, event))
        {
            return self;
        }

        if self.first.is_none() {
            self.first = Some(event);
        } else if self.second.is_none() {
            self.second = Some(event);
        }

        self
    }

    pub fn iter(self) -> impl Iterator<Item = &'a Event> {
        [self.first, self.second].into_iter().flatten()
    }
}

impl FileDescription {
    pub fn new(file: Arc<File>) -> Self {
        Self {
            file,
            close_on_exec: AtomicBool::new(false),
        }
    }
}

impl Clone for FileDescription {
    fn clone(&self) -> Self {
        Self {
            file: self.file.clone(),
            close_on_exec: AtomicBool::new(self.close_on_exec.load(Ordering::Acquire)),
        }
    }
}

/// Operations that can be performed on a file. Every trait function has a
/// generic implementation, which treats it as unimplemented.
/// Inputs have been sanitized when these functions are called.
pub trait FileOps: Sync + Send + Any {
    /// Called when the file is being opened.
    fn acquire(&self, file: &File, flags: OpenFlags) -> EResult<()> {
        let _ = (file, flags);
        Ok(())
    }

    /// Called when the file is being closed.
    fn release(&self, file: &File) -> EResult<()> {
        let _ = file;
        Ok(())
    }

    /// Reads from the file into a buffer.
    /// Returns actual bytes read and the new offset.
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let _ = (offset, buffer, file);
        Ok(0)
    }

    /// Writes a buffer to the file.
    /// Returns actual bytes written.
    fn write(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let _ = (offset, buffer, file);
        Ok(0)
    }

    /// Performs a generic ioctl operation on the file.
    /// Returns a driver specific status code if it was successful.
    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        _ = (arg, request, file);
        Err(Errno::ENOTTY)
    }

    /// Polls this file with a mask.
    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        _ = (file, mask);
        Ok(mask)
    }

    /// Returns the events that can change readiness for the requested poll mask.
    fn poll_events(&self, file: &File, mask: PollFlags) -> PollEventSet<'_> {
        let _ = (file, mask);
        PollEventSet::new()
    }

    /// Maps the file into an [`AddressSpace`].
    fn mmap(
        &self,
        file: &File,
        space: &mut AddressSpace,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        flags: MmapFlags,
        offset: uapi::off_t,
    ) -> EResult<VirtAddr> {
        let _ = (file, space, addr, len, prot, flags, offset);
        Err(Errno::ENODEV)
    }
}

impl File {
    /// Opens a file referenced by a path.
    pub fn open(
        root: PathNode,
        cwd: PathNode,
        path: &[u8],
        flags: OpenFlags,
        mode: Mode,
        identity: &Identity,
    ) -> EResult<Arc<Self>> {
        if flags.contains(OpenFlags::Directory)
            && flags.intersects(OpenFlags::Create | OpenFlags::Temporary)
        {
            return Err(Errno::EINVAL);
        }

        let mut lookup_flags = LookupFlags::empty();
        // If we don't want to create a file when opening, the lookup must succeed.
        // If we want to create a temporary unnamed file, `path` is the directory of the owning FS.
        // In that case the path also has to exist.
        if !flags.contains(OpenFlags::Create) || flags.contains(OpenFlags::Temporary) {
            lookup_flags |= LookupFlags::MustExist;
        }
        if flags.contains(OpenFlags::Exclusive) {
            lookup_flags |= LookupFlags::MustNotExist;
        }
        if !flags.intersects(OpenFlags::Exclusive | OpenFlags::NoFollow) {
            lookup_flags |= LookupFlags::FollowSymlinks;
        }

        let file_path = PathNode::lookup(root.clone(), cwd, path, identity, lookup_flags)?;
        match file_path.entry.get_inode() {
            Some(x) => Self::do_open_inode(file_path, &x, flags, identity),
            None => {
                // If the lookup was successful, we expect that the entry is positive.
                if !flags.contains(OpenFlags::Create) {
                    warn!("Tried opening a file without O_CREAT and backing inode, this is a bug!");
                    return Err(Errno::ENOENT);
                }

                let parent = file_path
                    .lookup_parent()
                    .and_then(|p| p.entry.get_inode().ok_or(Errno::ENOENT))
                    .expect("Entry should always have a parent");

                parent.try_access(identity, flags, false)?;

                match &parent.node_ops {
                    NodeOps::Directory(x) => {
                        x.create(&parent, file_path.entry.clone(), mode, identity)?;
                        file_path
                            .lookup_parent()?
                            .entry
                            .children
                            .lock()
                            .insert(file_path.entry.name.clone(), file_path.entry.clone())
                    }
                    _ => return Err(Errno::ENOTDIR),
                };

                let file_node = file_path.entry.get_inode().unwrap();
                Self::do_open_inode(file_path.clone(), &file_node, flags, identity)?;

                let result = File {
                    path: Some(file_path.clone()),
                    ops: file_node.file_ops(),
                    inode: Some(file_node),
                    flags: SpinMutex::new(flags),
                    offset: SpinMutex::new(0),
                };
                result.ops.acquire(&result, flags)?;
                Ok(Arc::try_new(result)?)
            }
        }
    }

    pub fn open_disconnected(ops: Arc<dyn FileOps>, flags: OpenFlags) -> EResult<Arc<File>> {
        let file = File {
            path: None,
            ops,
            inode: None,
            flags: SpinMutex::new(flags),
            offset: SpinMutex::new(0),
        };

        file.ops.acquire(&file, flags)?;
        Ok(Arc::try_new(file)?)
    }

    fn do_open_inode(
        file_path: PathNode,
        inode: &Arc<INode>,
        flags: OpenFlags,
        identity: &Identity,
    ) -> EResult<Arc<Self>> {
        // If we want to open as a directory, make sure this is actually a directory.
        if flags.contains(OpenFlags::Directory) {
            match &inode.node_ops {
                NodeOps::Directory(_) => {}
                _ => return Err(Errno::ENOTDIR),
            }
        }

        inode.try_access(identity, flags, false)?;
        let file = match &inode.node_ops {
            NodeOps::Regular(x) => {
                let result = File {
                    path: Some(file_path),
                    ops: x.clone(),
                    inode: Some(inode.clone()),
                    flags: SpinMutex::new(flags),
                    offset: SpinMutex::new(0),
                };
                Arc::try_new(result)?
            }
            NodeOps::Directory(dir) => dir.open(inode, file_path, flags, identity)?,
            NodeOps::BlockDevice(x) => {
                let result = File {
                    path: Some(file_path),
                    ops: x.clone(),
                    inode: Some(inode.clone()),
                    flags: SpinMutex::new(flags),
                    offset: SpinMutex::new(0),
                };
                Arc::try_new(result)?
            }
            NodeOps::CharacterDevice(x) => {
                let result = File {
                    path: Some(file_path),
                    ops: x.clone(),
                    inode: Some(inode.clone()),
                    flags: SpinMutex::new(flags),
                    offset: SpinMutex::new(0),
                };
                Arc::try_new(result)?
            }
            NodeOps::FIFO(_) => todo!(),
            NodeOps::SymbolicLink(_) => return Err(Errno::ELOOP),
            // Doesn't make sense to call open() on anything else.
            _ => return Err(Errno::ENOTSUP),
        };

        file.ops.acquire(&file, flags)?;
        Ok(file)
    }

    /// Reads into a buffer from a file.
    /// Returns actual bytes read.
    pub fn read(&self, buf: &mut IovecIter) -> EResult<isize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut offset = self.offset.lock();
        let read = self.ops.read(self, buf, *offset)?;
        *offset = offset.checked_add(read as u64).ok_or(Errno::EOVERFLOW)?;

        Ok(read)
    }

    /// Reads into a buffer from a file at a specified offset.
    /// Returns actual bytes read.
    pub fn pread(&self, buf: &mut IovecIter, offset: u64) -> EResult<isize> {
        if buf.is_empty() {
            return Ok(0);
        }

        self.ops.read(self, buf, offset)
    }

    pub fn pread_kernel(&self, buf: &mut [u8], offset: u64) -> EResult<isize> {
        let iovecs = [unsafe { IovecIter::iovec_from_mut_ptr(buf) }];
        let mut iter = unsafe { IovecIter::new_kernel(&iovecs) };
        self.pread(&mut iter, offset)
    }

    /// Writes a buffer to a file.
    /// Returns actual bytes written.
    pub fn write(&self, buf: &mut IovecIter) -> EResult<isize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut offset = self.offset.lock();
        let written = self.ops.write(self, buf, *offset)?;
        *offset = offset.checked_add(written as u64).ok_or(Errno::EOVERFLOW)?;

        Ok(written)
    }

    /// Writes a buffer to a file at a specified offset.
    /// Returns actual bytes written.
    pub fn pwrite(&self, buf: &mut IovecIter, offset: u64) -> EResult<isize> {
        if buf.is_empty() {
            return Ok(0);
        }

        self.ops.write(self, buf, offset)
    }

    pub fn pwrite_kernel(&self, buf: &[u8], offset: u64) -> EResult<isize> {
        let iovecs = [unsafe { IovecIter::iovec_from_ptr(buf) }];
        let mut iter = unsafe { IovecIter::new_kernel(&iovecs) };
        self.pwrite(&mut iter, offset)
    }

    pub fn poll(&self, mask: PollFlags) -> EResult<PollFlags> {
        self.ops.poll(self, mask)
    }

    pub fn seek(&self, offset: SeekAnchor) -> EResult<u64> {
        let mut position = self.offset.lock();

        match self.inode.as_ref().ok_or(Errno::ESPIPE)?.node_ops {
            NodeOps::CharacterDevice(_) | NodeOps::Socket(_) | NodeOps::FIFO(_) => {
                return Err(Errno::ESPIPE);
            }
            _ => (),
        }

        match offset {
            SeekAnchor::Start(x) => {
                *position = x;
                Ok(x)
            }
            SeekAnchor::Current(x) => {
                let new = position.checked_add_signed(x).ok_or(Errno::EOVERFLOW)?;
                *position = new;
                Ok(new)
            }
            SeekAnchor::End(x) => {
                let size = self.inode.as_ref().ok_or(Errno::EINVAL)?.size.lock();

                let new = if x.is_negative() {
                    size.checked_add_signed(x as _).ok_or(Errno::EINVAL)?
                } else {
                    size.checked_add_signed(x as _).ok_or(Errno::EOVERFLOW)?
                };

                *position = new as _;
                Ok(new as _)
            }
        }
    }

    pub fn mmap(
        &self,
        space: &mut AddressSpace,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        flags: MmapFlags,
        offset: uapi::off_t,
    ) -> EResult<VirtAddr> {
        self.ops.mmap(self, space, addr, len, prot, flags, offset)
    }

    pub fn ioctl(&self, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.ops.ioctl(self, request, arg)
    }

    pub fn close(&self) -> EResult<()> {
        self.ops.release(self)?;
        Ok(())
    }
}
