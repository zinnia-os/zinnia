use crate::{
    memory::{IovecIter, UserCStr, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::{
        dirent::dirent,
        fcntl::*,
        limits::PATH_MAX,
        mode_t,
        poll::{POLLERR, POLLNVAL, pollfd},
        stat::*,
        uio::iovec,
    },
    vfs::{
        self, File, MountFlags, PathNode,
        cache::LookupFlags,
        file::{FileDescription, OpenFlags, SeekAnchor},
        fs,
        inode::{INode, Mode, NodeOps},
    },
};
use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

pub fn pread(fd: i32, base: VirtAddr, len: usize, offset: usize) -> EResult<isize> {
    let file = {
        let proc = Scheduler::get_current().get_process();
        let proc_inner = proc.open_files.lock();
        proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file
    };

    if !file.flags.lock().contains(OpenFlags::Read) {
        return Err(Errno::EBADF);
    }

    let iovec = [iovec { base, len }];
    let mut iter = IovecIter::new(&iovec)?;
    file.pread(&mut iter, offset as _)
}

pub fn readv(fd: i32, iov: VirtAddr, iovcnt: usize) -> EResult<isize> {
    let file = {
        let proc = Scheduler::get_current().get_process();
        let proc_inner = proc.open_files.lock();
        proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file
    };

    if !file.flags.lock().contains(OpenFlags::Read) {
        return Err(Errno::EBADF);
    }

    let iov_ptr = UserPtr::<iovec>::new(iov);
    let mut iovecs = Vec::with_capacity(iovcnt);
    for i in 0..iovcnt {
        iovecs.push(iov_ptr.offset(i).read().ok_or(Errno::EFAULT)?);
    }

    let mut iter = IovecIter::new(&iovecs)?;
    file.read(&mut iter)
}

pub fn pwrite(fd: i32, base: VirtAddr, len: usize, offset: usize) -> EResult<isize> {
    let file = {
        let proc = Scheduler::get_current().get_process();
        let proc_inner = proc.open_files.lock();
        proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file
    };

    if !file.flags.lock().contains(OpenFlags::Write) {
        return Err(Errno::EBADF);
    }

    let iovec = [iovec { base, len }];
    let mut iter = IovecIter::new(&iovec)?;
    file.pwrite(&mut iter, offset as _)
}

pub fn writev(fd: i32, iov: VirtAddr, iovcnt: usize) -> EResult<isize> {
    let file = {
        let proc = Scheduler::get_current().get_process();
        let proc_inner = proc.open_files.lock();
        proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file
    };

    if !file.flags.lock().contains(OpenFlags::Write) {
        return Err(Errno::EBADF);
    }

    let iov_ptr = UserPtr::<iovec>::new(iov);
    let mut iovecs = Vec::with_capacity(iovcnt);
    for i in 0..iovcnt {
        iovecs.push(iov_ptr.offset(i).read().ok_or(Errno::EFAULT)?);
    }

    let mut iter = IovecIter::new(&iovecs)?;
    file.write(&mut iter)
}

pub fn openat(fd: i32, path: VirtAddr, oflag: usize /* mode */) -> EResult<i32> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let path = UserCStr::new(path);
    let v = path.as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;
    let oflag = OpenFlags::from_bits_truncate(oflag as _);

    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();
    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        proc_inner
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let file = File::open(
        proc.root_dir.lock().clone(),
        parent,
        &v,
        // O_CLOEXEC doesn't apply to a file, but rather its individual FD.
        // This means that dup'ing a file doesn't share this flag.
        oflag & !OpenFlags::CloseOnExec,
        Mode::empty(),
        &proc.identity.lock(),
    )?;

    proc_inner
        .open_file(
            FileDescription {
                file,
                close_on_exec: AtomicBool::new(oflag.contains(OpenFlags::CloseOnExec)),
            },
            0,
        )
        .ok_or(Errno::EMFILE)
}

pub fn seek(fd: i32, offset: usize, whence: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let file = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?.file;
    let anchor = match whence {
        0 => SeekAnchor::Start(offset as _),
        1 => SeekAnchor::Current(offset as _),
        2 => SeekAnchor::End(offset as _),
        _ => return Err(Errno::EINVAL),
    };
    file.seek(anchor).map(|x| x as _)
}

pub fn close(fd: i32) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();

    proc_inner.close(fd).ok_or(Errno::EBADF)?;
    Ok(0)
}

pub fn ioctl(fd: i32, request: usize, arg: VirtAddr) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();
    let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file;
    drop(proc_inner);

    file.ioctl(request, arg)
}

pub fn getcwd(user_buf: VirtAddr, len: usize) -> EResult<usize> {
    let mut user_buf = UserPtr::new(user_buf);
    let proc = Scheduler::get_current().get_process();

    let mut buffer = vec![0u8; PATH_MAX as _];
    let mut cursor = PATH_MAX;
    let mut current = proc.working_dir.lock().clone();

    // Walk up until we reach the root
    while let Ok(parent) = current.lookup_parent() {
        let name = &current.entry.name;
        if !name.is_empty() {
            // Copy name
            let len = name.len();
            cursor -= len;
            buffer[cursor..cursor + len].copy_from_slice(name);

            // Prepend slash
            cursor -= 1;
            buffer[cursor] = b'/';
        }
        current = parent;
    }

    // Special case: root directory
    if cursor == PATH_MAX {
        cursor -= 1;
        buffer[cursor] = b'/';
    }

    let path_len = buffer.len() - cursor;
    if path_len + 1 > len {
        return Err(Errno::ERANGE);
    }

    user_buf
        .write_slice(&buffer[cursor..])
        .ok_or(Errno::EFAULT)?;
    user_buf.offset(path_len).write(0).ok_or(Errno::EFAULT)?; // NUL terminator

    Ok(path_len)
}

fn write_stat(inode: &Arc<INode>, statbuf: &mut UserPtr<stat>) -> EResult<()> {
    statbuf
        .write(stat {
            st_dev: 0,
            st_ino: inode.id,
            st_mode: inode.mode.lock().bits()
                | match inode.node_ops {
                    NodeOps::Regular(_) => S_IFREG,
                    NodeOps::Directory(_) => S_IFDIR,
                    NodeOps::SymbolicLink(_) => S_IFLNK,
                    NodeOps::FIFO(_) => S_IFIFO,
                    NodeOps::BlockDevice(_) => S_IFBLK,
                    NodeOps::CharacterDevice(_) => S_IFCHR,
                    NodeOps::Socket(_) => S_IFSOCK,
                },
            st_nlink: Arc::strong_count(inode) as _,
            st_uid: *inode.uid.lock(),
            st_gid: *inode.gid.lock(),
            st_rdev: 0,
            st_size: *inode.size.lock() as _,
            st_atim: *inode.atime.lock(),
            st_mtim: *inode.mtime.lock(),
            st_ctim: *inode.ctime.lock(),
            st_blksize: 0,
            st_blocks: 0,
        })
        .ok_or(Errno::EFAULT)
}

pub fn fstat(fd: i32, statbuf: VirtAddr) -> EResult<()> {
    let mut statbuf = UserPtr::new(statbuf);
    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();

    let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file;
    let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;

    write_stat(inode, &mut statbuf)?;

    Ok(())
}

pub fn fstatat(at: i32, path: VirtAddr, statbuf: VirtAddr, flags: usize) -> EResult<()> {
    let mut statbuf: UserPtr<stat> = UserPtr::new(statbuf);
    let path = UserCStr::new(path);
    let v = path.as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();
    let parent = if at == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        proc_inner
            .get_fd(at)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let node = PathNode::lookup(
        proc.root_dir.lock().clone(),
        parent,
        &v,
        &proc.identity.lock(),
        LookupFlags::MustExist
            | if (flags & (AT_SYMLINK_NOFOLLOW as usize)) != 0 {
                LookupFlags::empty()
            } else {
                LookupFlags::FollowSymlinks
            },
    )?;
    let inode = node.entry.get_inode().ok_or(Errno::EINVAL)?;

    drop(proc_inner);
    write_stat(&inode, &mut statbuf)?;

    Ok(())
}

pub fn dup(fd: i32) -> EResult<i32> {
    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();
    let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
    proc_inner.open_file(file, fd).ok_or(Errno::EMFILE)
}

pub fn dup3(fd1: i32, fd2: i32, flags: usize) -> EResult<i32> {
    if fd1 == fd2 {
        return Ok(fd1);
    }

    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();

    let file = proc_inner.get_fd(fd1).ok_or(Errno::EBADF)?;
    if proc_inner.get_fd(fd2).is_some() {
        proc_inner.close(fd2);
    }

    let flags = OpenFlags::from_bits_truncate(flags as _);
    if flags.contains(OpenFlags::CloseOnExec) {
        file.close_on_exec.store(true, Ordering::Release);
    }

    proc_inner.open_file(file, fd2).ok_or(Errno::EMFILE)
}

pub fn mkdirat(fd: i32, path: VirtAddr, mode: mode_t) -> EResult<i32> {
    let path = UserCStr::new(path);
    let v = path.as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let inner = proc.open_files.lock();
    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        inner
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };
    vfs::mkdir(
        proc.root_dir.lock().clone(),
        parent,
        &v,
        Mode::from_bits(mode).ok_or(Errno::EINVAL)?,
        &proc.identity.lock(),
    )?;

    Ok(0)
}

pub fn chdir(path: VirtAddr) -> EResult<()> {
    let path = UserCStr::new(path);
    let v = path.as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let root = proc.root_dir.lock();
    let mut cwd = proc.working_dir.lock();
    let node = PathNode::lookup(
        root.clone(),
        cwd.clone(),
        &v,
        &proc.identity.lock(),
        LookupFlags::MustExist,
    )?;
    *cwd = node;

    Ok(())
}

pub fn fchdir(fd: i32) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut cwd = proc.working_dir.lock();
    let dir = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?;
    *cwd = dir.file.path.as_ref().cloned().ok_or(Errno::ENOTDIR)?;

    Ok(())
}

pub fn getdents(fd: i32, addr: VirtAddr, len: usize) -> EResult<usize> {
    if len == 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let inner = proc.open_files.lock();

    // fd must be a valid descriptor open for reading.
    let dir = inner.get_fd(fd).ok_or(Errno::EBADF)?.file;
    let flags = *dir.flags.lock();
    if !flags.contains(OpenFlags::Read | OpenFlags::Directory) {
        return Err(Errno::EBADF);
    };

    let mut buffer = vec![
        dirent {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0u8; _]
        };
        len / size_of::<dirent>()
    ];

    let to_write = vfs::get_dir_entries(dir, &mut buffer, &proc.identity.lock())?;
    let mut addr = UserPtr::new(addr);
    addr.write_slice(&buffer[0..to_write])
        .ok_or(Errno::EFAULT)?;

    Ok(to_write * size_of::<dirent>())
}

pub fn fcntl(fd: i32, cmd: usize, arg: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();

    match cmd as _ {
        F_DUPFD => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            proc_inner
                .open_file(file, arg as i32)
                .ok_or(Errno::EMFILE)
                .map(|x| x as usize)
        }
        F_DUPFD_CLOEXEC => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            file.close_on_exec.store(true, Ordering::Release);
            proc_inner
                .open_file(file, arg as i32)
                .ok_or(Errno::EMFILE)
                .map(|x| x as usize)
        }
        F_GETFD => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            let mut flags = OpenFlags::empty();
            flags.set(
                OpenFlags::CloseOnExec,
                file.close_on_exec.load(Ordering::Acquire),
            );
            Ok(flags.bits() as _)
        }
        F_SETFD => Ok(0),
        F_GETFL => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            let flags = *file.file.flags.lock();
            Ok(flags.bits() as _)
        }
        F_SETFL => {
            warn!("fcntl F_SETFL is a stub!");
            Ok(0)
        }
        F_GETOWN => {
            warn!("fcntl F_GETOWN is a stub!");
            Ok(0)
        }
        F_SETOWN => {
            warn!("fcntl F_SETOWN is a stub!");
            Ok(0)
        }
        F_GETOWN_EX => {
            warn!("fcntl F_GETOWN_EX is a stub!");
            Ok(0)
        }
        F_SETOWN_EX => {
            warn!("fcntl F_SETOWN_EX is a stub!");
            Ok(0)
        }
        F_GETLK => {
            warn!("fcntl F_GETLK is a stub!");
            Ok(0)
        }
        F_SETLK => {
            warn!("fcntl F_SETLK is a stub!");
            Ok(0)
        }
        F_SETLKW => {
            warn!("fcntl F_SETLKW is a stub!");
            Ok(0)
        }
        F_OFD_GETLK => {
            warn!("fcntl F_OFD_GETLK is a stub!");
            Ok(0)
        }
        F_OFD_SETLK => {
            warn!("fcntl F_OFD_SETLK is a stub!");
            Ok(0)
        }
        F_OFD_SETLKW => {
            warn!("fcntl F_OFD_SETLKW is a stub!");
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}

pub fn ppoll(
    fds_ptr: VirtAddr,
    nfds: usize,
    timeout_ptr: VirtAddr,
    sigmask_ptr: VirtAddr,
) -> EResult<usize> {
    // Read the pollfd array from userspace
    let fds_ptr = UserPtr::<pollfd>::new(fds_ptr);
    let mut fds = vec![
        pollfd {
            fd: 0,
            events: 0,
            revents: 0,
        };
        nfds
    ];
    fds_ptr.read_slice(&mut fds).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();

    let mut ready_count = 0;

    // Poll each file descriptor
    for poll_entry in fds.iter_mut() {
        let fd = poll_entry.fd;
        poll_entry.revents = 0;

        if fd < 0 {
            // Negative fd means ignore this entry
            continue;
        }

        // Get the file
        let file_desc = match proc_inner.get_fd(fd) {
            Some(f) => f,
            None => {
                // Invalid fd - set POLLNVAL
                poll_entry.revents = POLLNVAL;
                ready_count += 1;
                continue;
            }
        };

        // Call the file's poll method
        match file_desc.file.poll(poll_entry.events) {
            Ok(revents) => {
                poll_entry.revents = revents;
                if revents != 0 {
                    ready_count += 1;
                }
            }
            Err(_) => {
                poll_entry.revents = POLLERR;
                ready_count += 1;
            }
        }
    }

    // For now, we only support non-blocking poll (ignore timeout and sigmask)
    // TODO: Implement blocking with timeout
    let _ = (timeout_ptr, sigmask_ptr);
    Ok(ready_count)
}

pub fn pipe(filedes: VirtAddr) -> EResult<()> {
    let mut filedes = UserPtr::<[i32; 2]>::new(filedes);
    let fds = {
        let proc = Scheduler::get_current().get_process();
        let mut files = proc.open_files.lock();
        let (pipe1, pipe2) = vfs::pipe()?;
        [
            files
                .open_file(
                    FileDescription {
                        file: pipe1,
                        close_on_exec: AtomicBool::new(false),
                    },
                    0,
                )
                .ok_or(Errno::EMFILE)? as _,
            files
                .open_file(
                    FileDescription {
                        file: pipe2,
                        close_on_exec: AtomicBool::new(false),
                    },
                    0,
                )
                .ok_or(Errno::EMFILE)? as _,
        ]
    };

    filedes.write(fds).ok_or(Errno::EFAULT)
}

pub fn faccessat(fd: i32, path: VirtAddr, amode: usize, flag: usize) -> EResult<()> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let path = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();
    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        proc_inner
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let path_node = PathNode::lookup(
        proc.root_dir.lock().clone(),
        parent,
        &path,
        &proc.identity.lock(),
        LookupFlags::MustExist
            | LookupFlags::FollowSymlinks
            | if flag as u32 & AT_EACCESS != 0 {
                LookupFlags::empty()
            } else {
                LookupFlags::UseRealId
            },
    )?;

    let node = path_node.entry.get_inode().ok_or(Errno::EBADF)?;
    let amode = Mode::from_bits_truncate(amode as _);
    if !node.mode.lock().intersects(amode) {
        return Err(Errno::EACCES);
    }

    Ok(())
}

pub fn unlinkat(fd: i32, path: VirtAddr, flags: usize) -> EResult<()> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let buf = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    warn!(
        "unlinkat({}, \"{}\", {:#x}) is a stub!",
        fd,
        String::from_utf8_lossy(&buf),
        flags
    );

    Ok(())
}

pub fn linkat(
    old_fd: i32,
    old_path: VirtAddr,
    new_fd: i32,
    new_path: VirtAddr,
    flags: usize,
) -> EResult<()> {
    if old_path == VirtAddr::null() || new_path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let old_path = UserCStr::new(old_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;
    let new_path = UserCStr::new(new_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;

    warn!(
        "linkat({}, \"{}\", {}, \"{}\", {:#x}) is a stub!",
        old_fd,
        String::from_utf8_lossy(&old_path),
        new_fd,
        String::from_utf8_lossy(&new_path),
        flags,
    );

    Ok(())
}

pub fn readlinkat(at: i32, path: VirtAddr, buf: VirtAddr, buf_len: usize) -> EResult<isize> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();
    let at = if at == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(at)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let path = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EINVAL)?;
    let node = PathNode::lookup(
        proc.root_dir.lock().clone(),
        at,
        &path,
        &proc.identity.lock(),
        LookupFlags::MustExist,
    )?;
    let inode = node.entry.get_inode().ok_or(Errno::EBADF)?;
    let ops = match &inode.node_ops {
        NodeOps::SymbolicLink(x) => x,
        _ => return Err(Errno::EINVAL)?,
    };

    let mut result = vec![0u8; buf_len];
    let read = ops.read_link(&inode, &mut result)?;

    let mut buf = UserPtr::new(buf);
    buf.write_slice(&result[0..(read as usize)])
        .ok_or(Errno::EFAULT)?;

    Ok(read as _)
}

pub fn mount(
    type_ptr: VirtAddr,
    dir_ptr: VirtAddr,
    flags: u32,
    data_ptr: VirtAddr,
) -> EResult<usize> {
    let fs_type = UserCStr::new(type_ptr)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;
    let dir = UserCStr::new(dir_ptr)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;

    let mount_flags = MountFlags::from_bits_truncate(flags);

    let proc = Scheduler::get_current().get_process();
    let root = proc.root_dir.lock().clone();
    let cwd = proc.working_dir.lock().clone();
    let identity = proc.identity.lock().clone();

    let mount_point = PathNode::lookup(
        root.clone(),
        cwd,
        &dir,
        &identity,
        LookupFlags::MustExist | LookupFlags::FollowSymlinks,
    )?;

    let new_mount = fs::mount(&fs_type, mount_flags, UserPtr::new(data_ptr))?;

    mount_point.mount(new_mount)?;
    Ok(0)
}

pub fn chroot(path: VirtAddr) -> EResult<usize> {
    let path = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let root = proc.root_dir.lock().clone();
    let cwd = proc.working_dir.lock().clone();
    let identity = proc.identity.lock().clone();

    let node = PathNode::lookup(
        root,
        cwd,
        &path,
        &identity,
        LookupFlags::MustExist | LookupFlags::FollowSymlinks,
    )?;

    // Verify it's a directory.
    let inode = node.entry.get_inode().ok_or(Errno::ENOENT)?;
    match &inode.node_ops {
        NodeOps::Directory(_) => {}
        _ => return Err(Errno::ENOTDIR),
    }

    *proc.root_dir.lock() = node.clone();
    *proc.working_dir.lock() = node;
    Ok(0)
}

pub fn umount(dir_ptr: VirtAddr, _flags: u32) -> EResult<usize> {
    let dir = UserCStr::new(dir_ptr)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let root = proc.root_dir.lock().clone();
    let cwd = proc.working_dir.lock().clone();
    let identity = proc.identity.lock().clone();

    let mount_point = PathNode::lookup(
        root,
        cwd,
        &dir,
        &identity,
        LookupFlags::MustExist | LookupFlags::FollowSymlinks,
    )?;

    // Remove the last mount from this entry's mount list.
    let mut mounts = mount_point.entry.mounts.lock();
    if mounts.is_empty() {
        return Err(Errno::EINVAL);
    }
    mounts.pop();
    Ok(0)
}
