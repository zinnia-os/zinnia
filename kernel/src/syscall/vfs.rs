use crate::{
    clock,
    device::net::Socket,
    memory::{IovecIter, UserCStr, VirtAddr, user::UserPtr},
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::signal::SignalSet,
    sched::Scheduler,
    uapi::{
        access::{R_OK, W_OK, X_OK},
        dirent::dirent,
        epoll::{
            EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLET, EPOLLEXCLUSIVE, EPOLLMSG,
            EPOLLONESHOT, epoll_data, epoll_event,
        },
        fcntl::{O_CLOEXEC, *},
        limits::PATH_MAX,
        mode_t,
        poll::pollfd,
        signal::{SFD_CLOEXEC, SFD_NONBLOCK, sigset_t},
        stat::*,
        statvfs::statvfs,
        time::{TFD_CLOEXEC, TFD_NONBLOCK, TFD_TIMER_ABSTIME, itimerspec},
        uio::iovec,
    },
    vfs::{
        self, File, MountFlags, PathNode,
        cache::LookupFlags,
        epoll::EpollFile,
        file::{FileDescription, FileOps, OpenFlags, PollFlags, SeekAnchor},
        fs,
        inode::{INode, Mode, NodeOps},
        pipe::PipeBuffer,
        signalfd::SignalfdFile,
        timerfd::{self, TimerfdFile},
    },
    wrap_syscall,
};
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

#[wrap_syscall]
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

#[wrap_syscall]
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

#[wrap_syscall]
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

#[wrap_syscall]
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

#[wrap_syscall]
pub fn openat(fd: i32, path: UserCStr, oflag: usize, mode: mode_t) -> EResult<i32> {
    if path.is_null() {
        return Err(Errno::EINVAL);
    }

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

    let umask = proc.umask.load(core::sync::atomic::Ordering::Relaxed);
    let file = File::open(
        proc.root_dir.lock().clone(),
        parent,
        &v,
        // O_CLOEXEC doesn't apply to a file, but rather its individual FD.
        // This means that dup'ing a file doesn't share this flag.
        oflag & !OpenFlags::CloseOnExec,
        Mode::from_bits_truncate(mode & !umask),
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

#[wrap_syscall]
pub fn seek(fd: i32, offset: usize, whence: i32) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let file = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?.file;
    let anchor = match whence {
        SEEK_SET => SeekAnchor::Start(offset as _),
        SEEK_CUR => SeekAnchor::Current(offset as _),
        SEEK_END => SeekAnchor::End(offset as _),
        _ => return Err(Errno::EINVAL),
    };
    file.seek(anchor).map(|x| x as _)
}

#[wrap_syscall]
pub fn close(fd: i32) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();

    proc_inner.close(fd).ok_or(Errno::EBADF)?;
    Ok(0)
}

#[wrap_syscall]
pub fn ioctl(fd: i32, request: usize, arg: VirtAddr) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();
    let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file;
    drop(proc_inner);

    file.ioctl(request, arg)
}

#[wrap_syscall]
pub fn getcwd(user_buf: VirtAddr, len: usize) -> EResult<usize> {
    let mut user_buf = UserPtr::new(user_buf);
    let proc = Scheduler::get_current().get_process();

    let mut buffer = vec![0u8; PATH_MAX as _];
    let mut cursor = PATH_MAX;
    let mut current = proc.working_dir.lock().clone();
    let root = proc.root_dir.lock().clone();
    let mut reached_root = false;

    // Walk up until we reach this process' root.
    loop {
        if Arc::ptr_eq(&current.entry, &root.entry) && Arc::ptr_eq(&current.mount, &root.mount) {
            reached_root = true;
            break;
        }

        let Ok(parent) = current.lookup_parent() else {
            break;
        };

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

    if !reached_root {
        return Err(Errno::ENOENT);
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

    Ok(path_len + 1)
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
            // Use the inode number as a unique devid for char/block devices,
            // so that userspace can distinguish between distinct device nodes.
            st_rdev: match inode.node_ops {
                NodeOps::CharacterDevice(_) | NodeOps::BlockDevice(_) => inode.id,
                _ => 0,
            },
            st_size: *inode.size.lock() as _,
            st_atim: *inode.atime.lock(),
            st_mtim: *inode.mtime.lock(),
            st_ctim: *inode.ctime.lock(),
            st_blksize: 0,
            st_blocks: 0,
        })
        .ok_or(Errno::EFAULT)
}

#[wrap_syscall]
pub fn fstat(fd: i32, statbuf: VirtAddr) -> EResult<()> {
    let mut statbuf = UserPtr::new(statbuf);
    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();

    let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file;
    if let Some(inode) = file.inode.as_ref() {
        write_stat(inode, &mut statbuf)?;
    } else {
        // Files without INodes need a fake stat.
        let ops_any = file.ops.as_ref() as &dyn core::any::Any;
        let mode = if ops_any.is::<PipeBuffer>() {
            S_IFIFO
        } else if ops_any.is::<Socket>() {
            S_IFSOCK
        } else {
            S_IFIFO
        };
        statbuf
            .write(stat {
                st_mode: mode,
                st_nlink: 1,
                ..Default::default()
            })
            .ok_or(Errno::EFAULT)?;
    }

    Ok(())
}

#[wrap_syscall]
pub fn fstatat(at: i32, path: UserCStr, statbuf: VirtAddr, flags: usize) -> EResult<()> {
    let mut statbuf: UserPtr<stat> = UserPtr::new(statbuf);
    if path.is_null() {
        return Err(Errno::EINVAL);
    }

    if flags & !((AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) as usize) != 0 {
        return Err(Errno::EINVAL);
    }

    let v = path.as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();

    if v.is_empty() && flags & (AT_EMPTY_PATH as usize) != 0 {
        let inode = if at == AT_FDCWD as _ {
            proc.working_dir
                .lock()
                .entry
                .get_inode()
                .ok_or(Errno::ENOENT)?
        } else {
            proc_inner
                .get_fd(at)
                .ok_or(Errno::EBADF)?
                .file
                .inode
                .as_ref()
                .ok_or(Errno::EINVAL)?
                .clone()
        };

        drop(proc_inner);
        return write_stat(&inode, &mut statbuf);
    }

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

#[wrap_syscall]
pub fn dup(fd: i32) -> EResult<i32> {
    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();
    let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
    proc_inner
        .open_file(file.duplicate(false), 0)
        .ok_or(Errno::EMFILE)
}

#[wrap_syscall]
pub fn flock(fd: i32, _operation: usize) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?;
    // TODO
    Ok(())
}

#[wrap_syscall]
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
    let file = file.duplicate(flags.contains(OpenFlags::CloseOnExec));

    proc_inner.open_file(file, fd2).ok_or(Errno::EMFILE)
}

#[wrap_syscall]
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
    let umask = proc.umask.load(core::sync::atomic::Ordering::Relaxed);
    vfs::mkdir(
        proc.root_dir.lock().clone(),
        parent,
        &v,
        Mode::from_bits_truncate(mode & !umask),
        &proc.identity.lock(),
    )?;

    Ok(0)
}

#[wrap_syscall]
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
        LookupFlags::MustExist | LookupFlags::FollowSymlinks,
    )?;
    let inode = node.entry.get_inode().ok_or(Errno::ENOENT)?;
    if !matches!(&inode.node_ops, NodeOps::Directory(_)) {
        return Err(Errno::ENOTDIR);
    }

    *cwd = node;

    Ok(())
}

#[wrap_syscall]
pub fn fchdir(fd: i32) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut cwd = proc.working_dir.lock();
    let dir = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?;
    let path = dir.file.path.as_ref().cloned().ok_or(Errno::ENOTDIR)?;
    let inode = path.entry.get_inode().ok_or(Errno::ENOTDIR)?;
    if !matches!(&inode.node_ops, NodeOps::Directory(_)) {
        return Err(Errno::ENOTDIR);
    }

    *cwd = path;

    Ok(())
}

#[wrap_syscall]
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

#[wrap_syscall]
pub fn fcntl(fd: i32, cmd: usize, arg: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let mut proc_inner = proc.open_files.lock();

    match cmd as _ {
        F_DUPFD => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            proc_inner
                .open_file(file.duplicate(false), arg as i32)
                .ok_or(Errno::EMFILE)
                .map(|x| x as usize)
        }
        F_DUPFD_CLOEXEC => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            proc_inner
                .open_file(file.duplicate(true), arg as i32)
                .ok_or(Errno::EMFILE)
                .map(|x| x as usize)
        }
        F_GETFD => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            if file.close_on_exec.load(Ordering::Acquire) {
                Ok(FD_CLOEXEC as _)
            } else {
                Ok(0)
            }
        }
        F_SETFD => {
            proc_inner
                .set_close_on_exec(fd, arg as u32 & FD_CLOEXEC != 0)
                .ok_or(Errno::EBADF)?;
            Ok(0)
        }
        F_GETFL => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            let flags = *file.file.flags.lock();
            Ok(flags.bits() as _)
        }
        F_SETFL => {
            let file = proc_inner.get_fd(fd).ok_or(Errno::EBADF)?;
            let new_flags = OpenFlags::from_bits_truncate(arg as u32);
            // Only status flags (NonBlocking, Append) can be changed via F_SETFL.
            let changeable = OpenFlags::NonBlocking | OpenFlags::Append;
            let mut flags = file.file.flags.lock();
            flags.remove(changeable);
            flags.insert(new_flags & changeable);
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

#[wrap_syscall]
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

    // Determine if this is a non-blocking poll (timeout of zero).
    let is_nonblocking = if !timeout_ptr.is_null() {
        let ts: UserPtr<crate::uapi::time::timespec> = UserPtr::new(timeout_ptr);
        let timeout = ts.read().ok_or(Errno::EFAULT)?;
        timeout.tv_sec == 0 && timeout.tv_nsec == 0
    } else {
        false // NULL timeout = block indefinitely
    };

    let _ = sigmask_ptr; // TODO: apply signal mask during poll

    let proc = Scheduler::get_current().get_process();

    // Collect the Arc<File> references for each valid fd once (avoids holding
    // the open_files lock across a potential block).
    let files: Vec<Option<Arc<File>>> = {
        let open_files = proc.open_files.lock();
        fds.iter()
            .map(|e| {
                if e.fd < 0 {
                    None
                } else {
                    open_files.get_fd(e.fd).map(|d| d.file)
                }
            })
            .collect()
    };

    // Register as a waiter on every fd that provides a poll event before the
    // first poll pass so we don't miss a wake-up that happens between the poll
    // check and going to sleep.
    let _guards: Vec<_> = if is_nonblocking {
        Vec::new()
    } else {
        let mut guards = Vec::new();

        for (poll_entry, file_opt) in fds.iter().zip(files.iter()) {
            if let Some(file) = file_opt {
                let mask = PollFlags::from_bits_truncate(poll_entry.events);
                guards.extend(
                    file.ops
                        .poll_events(file, mask)
                        .iter()
                        .map(|event| event.guard()),
                );
            }
        }

        guards
    };

    loop {
        let mut ready_count = 0usize;

        for (poll_entry, file_opt) in fds.iter_mut().zip(files.iter()) {
            poll_entry.revents = 0;

            if poll_entry.fd < 0 {
                continue;
            }

            let file = match file_opt {
                Some(f) => f,
                None => {
                    poll_entry.revents = PollFlags::Nval.bits();
                    ready_count += 1;
                    continue;
                }
            };

            let mask = PollFlags::from_bits_truncate(poll_entry.events);

            match file.poll(mask) {
                Ok(revents) => {
                    poll_entry.revents = revents.bits();
                    if !revents.is_empty() {
                        ready_count += 1;
                    }
                }
                Err(_) => {
                    poll_entry.revents = PollFlags::Err.bits();
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 || is_nonblocking {
            // Write back the results.
            let mut fds_out = UserPtr::<pollfd>::new(fds_ptr.addr());
            fds_out.write_slice(&fds).ok_or(Errno::EFAULT)?;
            return Ok(ready_count);
        }

        // Nothing ready — block until a file signals readiness.
        // The EventGuards we already hold ensure we'll be woken up.
        // If no guards were collected (none of the fds have a poll_event),
        // return immediately to avoid hanging forever.
        if _guards.is_empty() {
            let mut fds_out = UserPtr::<pollfd>::new(fds_ptr.addr());
            fds_out.write_slice(&fds).ok_or(Errno::EFAULT)?;
            return Ok(0);
        }

        _guards[0].wait();
        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::EINTR);
        }
    }
}

#[wrap_syscall]
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

#[wrap_syscall]
pub fn faccessat(fd: i32, path: UserCStr, amode: usize, flag: usize) -> EResult<()> {
    if path.is_null() {
        return Err(Errno::EINVAL);
    }

    let path = path.as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;
    if amode & !((R_OK | W_OK | X_OK) as usize) != 0 {
        return Err(Errno::EINVAL);
    }
    if flag & !((AT_EACCESS | AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) as usize) != 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let proc_inner = proc.open_files.lock();
    let inode = if path.is_empty() && flag as u32 & AT_EMPTY_PATH != 0 {
        if fd == AT_FDCWD as _ {
            proc.working_dir
                .lock()
                .entry
                .get_inode()
                .ok_or(Errno::ENOENT)?
        } else {
            proc_inner
                .get_fd(fd)
                .ok_or(Errno::EBADF)?
                .file
                .inode
                .clone()
                .ok_or(Errno::EBADF)?
        }
    } else {
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
                | if flag as u32 & AT_SYMLINK_NOFOLLOW != 0 {
                    LookupFlags::empty()
                } else {
                    LookupFlags::FollowSymlinks
                }
                | if flag as u32 & AT_EACCESS != 0 {
                    LookupFlags::empty()
                } else {
                    LookupFlags::UseRealId
                },
        )?;

        path_node.entry.get_inode().ok_or(Errno::ENOENT)?
    };

    let mut access_flags = OpenFlags::empty();
    if amode & (R_OK as usize) != 0 {
        access_flags |= OpenFlags::Read;
    }
    if amode & (W_OK as usize) != 0 {
        access_flags |= OpenFlags::Write;
    }
    if amode & (X_OK as usize) != 0 {
        access_flags |= OpenFlags::Executable;
    }
    inode.try_access(
        &proc.identity.lock(),
        access_flags,
        flag as u32 & AT_EACCESS == 0,
    )?;

    Ok(())
}

#[wrap_syscall]
pub fn statvfs(path: VirtAddr, buf: VirtAddr) -> EResult<()> {
    let mut buf: UserPtr<statvfs> = UserPtr::new(buf);
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
    let inode = node.entry.get_inode().ok_or(Errno::EINVAL)?;
    let sb = inode.sb.as_ref().ok_or(Errno::ENOSYS)?;

    let result = sb.clone().statvfs()?;
    buf.write(result).ok_or(Errno::EFAULT)
}

#[wrap_syscall]
pub fn fstatvfs(fd: i32, buf: VirtAddr) -> EResult<()> {
    let mut buf: UserPtr<statvfs> = UserPtr::new(buf);
    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();

    let file = files.get_fd(fd).ok_or(Errno::EBADF)?.file;
    let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
    let sb = inode.sb.as_ref().ok_or(Errno::ENOSYS)?;

    let result = sb.clone().statvfs()?;
    buf.write(result).ok_or(Errno::EFAULT)
}

#[wrap_syscall]
pub fn renameat(old_fd: i32, old_path: VirtAddr, new_fd: i32, new_path: VirtAddr) -> EResult<()> {
    if old_path == VirtAddr::null() || new_path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let old_path_buf = UserCStr::new(old_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;
    let new_path_buf = UserCStr::new(new_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();

    let old_parent = if old_fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(old_fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let new_parent = if new_fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(new_fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let root = proc.root_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    drop(files);

    let old_node = PathNode::lookup(
        root.clone(),
        old_parent,
        &old_path_buf,
        &identity,
        LookupFlags::MustExist,
    )?;

    let old_parent_node = old_node.lookup_parent()?;
    let old_parent_inode = old_parent_node.entry.get_inode().ok_or(Errno::ENOENT)?;
    old_parent_inode.try_access(&identity, OpenFlags::Write, false)?;

    let new_node = PathNode::lookup(
        root,
        new_parent,
        &new_path_buf,
        &identity,
        LookupFlags::empty(),
    )?;

    let new_parent_node = new_node.lookup_parent()?;
    let new_parent_inode = new_parent_node.entry.get_inode().ok_or(Errno::ENOENT)?;
    new_parent_inode.try_access(&identity, OpenFlags::Write, false)?;

    match &old_parent_inode.node_ops {
        NodeOps::Directory(x) => x.rename(
            &old_parent_inode,
            old_node,
            &new_parent_inode,
            new_node,
            &identity,
        ),
        _ => Err(Errno::ENOTDIR),
    }
}

#[wrap_syscall]
pub fn fchmod(fd: i32, mode: mode_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();

    let file = files.get_fd(fd).ok_or(Errno::EBADF)?.file;
    let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
    inode.chmod(Mode::from_bits_truncate(mode));
    Ok(())
}

#[wrap_syscall]
pub fn ftruncate(fd: i32, length: i64) -> EResult<()> {
    if length < 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let file = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?.file;

    if !file.flags.lock().contains(OpenFlags::Write) {
        return Err(Errno::EINVAL);
    }

    let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
    match &inode.node_ops {
        NodeOps::Regular(ops) => ops.truncate(inode, length as u64),
        _ => Err(Errno::EINVAL),
    }
}

#[wrap_syscall]
pub fn fallocate(fd: i32, offset: i64, length: i64) -> EResult<()> {
    if offset < 0 || length <= 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let file = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?.file;

    if !file.flags.lock().contains(OpenFlags::Write) {
        return Err(Errno::EBADF);
    }

    let inode = file.inode.as_ref().ok_or(Errno::EINVAL)?;
    let NodeOps::Regular(ops) = &inode.node_ops else {
        return Err(Errno::ENODEV);
    };

    let required = (offset as u64).saturating_add(length as u64);
    if required > inode.len() as u64 {
        ops.truncate(inode, required)?;
    }
    Ok(())
}

#[wrap_syscall]
pub fn fchmodat(fd: i32, path: VirtAddr, mode: mode_t, flags: usize) -> EResult<()> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    if flags & !((AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) as usize) != 0 {
        return Err(Errno::EINVAL);
    }

    let path_buf = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();

    if path_buf.is_empty() && flags & (AT_EMPTY_PATH as usize) != 0 {
        let inode = if fd == AT_FDCWD as _ {
            proc.working_dir
                .lock()
                .entry
                .get_inode()
                .ok_or(Errno::ENOENT)?
        } else {
            files
                .get_fd(fd)
                .ok_or(Errno::EBADF)?
                .file
                .inode
                .as_ref()
                .ok_or(Errno::EINVAL)?
                .clone()
        };

        inode.chmod(Mode::from_bits_truncate(mode));
        return Ok(());
    }

    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let root = proc.root_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    drop(files);

    let node = PathNode::lookup(
        root,
        parent,
        &path_buf,
        &identity,
        LookupFlags::MustExist
            | if (flags & AT_SYMLINK_NOFOLLOW as usize) != 0 {
                LookupFlags::empty()
            } else {
                LookupFlags::FollowSymlinks
            },
    )?;
    let inode = node.entry.get_inode().ok_or(Errno::EINVAL)?;
    inode.chmod(Mode::from_bits_truncate(mode));
    Ok(())
}

#[wrap_syscall]
pub fn fchownat(fd: i32, path: VirtAddr, uid: u32, gid: u32, flags: usize) -> EResult<()> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    if flags & !((AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) as usize) != 0 {
        return Err(Errno::EINVAL);
    }

    let path_buf = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();

    if path_buf.is_empty() && flags & (AT_EMPTY_PATH as usize) != 0 {
        let inode = if fd == AT_FDCWD as _ {
            proc.working_dir
                .lock()
                .entry
                .get_inode()
                .ok_or(Errno::ENOENT)?
        } else {
            files
                .get_fd(fd)
                .ok_or(Errno::EBADF)?
                .file
                .inode
                .as_ref()
                .ok_or(Errno::EINVAL)?
                .clone()
        };

        inode.chown(uid, gid);
        return Ok(());
    }

    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let root = proc.root_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    drop(files);

    let node = PathNode::lookup(
        root,
        parent,
        &path_buf,
        &identity,
        LookupFlags::MustExist
            | if (flags & AT_SYMLINK_NOFOLLOW as usize) != 0 {
                LookupFlags::empty()
            } else {
                LookupFlags::FollowSymlinks
            },
    )?;
    let inode = node.entry.get_inode().ok_or(Errno::EINVAL)?;
    inode.chown(uid, gid);
    Ok(())
}

#[wrap_syscall]
pub fn unlinkat(fd: i32, path: VirtAddr, flags: usize) -> EResult<()> {
    if path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    if flags & !(AT_REMOVEDIR as usize) != 0 {
        return Err(Errno::EINVAL);
    }

    if flags & (AT_REMOVEDIR as usize) != 0 {
        return Err(Errno::ENOSYS);
    }

    let path_buf = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();
    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let root = proc.root_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    drop(files);

    vfs::unlink(root, parent, &path_buf, &identity)
}

#[wrap_syscall]
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

    let old_path_buf = UserCStr::new(old_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;
    let new_path_buf = UserCStr::new(new_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();

    let old_parent = if old_fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(old_fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let new_parent = if new_fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(new_fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let root = proc.root_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    drop(files);

    let follow = if (flags & AT_SYMLINK_FOLLOW as usize) != 0 {
        LookupFlags::FollowSymlinks
    } else {
        LookupFlags::empty()
    };

    let old_node = PathNode::lookup(
        root.clone(),
        old_parent,
        &old_path_buf,
        &identity,
        LookupFlags::MustExist | follow,
    )?;
    let target_inode = old_node.entry.get_inode().ok_or(Errno::ENOENT)?;

    // Hard links to directories are not allowed.
    if matches!(target_inode.node_ops, NodeOps::Directory(_)) {
        return Err(Errno::EPERM);
    }

    let new_node = PathNode::lookup(
        root,
        new_parent,
        &new_path_buf,
        &identity,
        LookupFlags::MustNotExist,
    )?;

    let new_parent_node = new_node.lookup_parent()?;
    let new_parent_inode = new_parent_node.entry.get_inode().ok_or(Errno::ENOENT)?;
    new_parent_inode.try_access(&identity, OpenFlags::Write, false)?;

    match &new_parent_inode.node_ops {
        NodeOps::Directory(x) => x.link(&new_parent_inode, &new_node, &target_inode, &identity),
        _ => Err(Errno::ENOTDIR),
    }
}

#[wrap_syscall]
pub fn symlinkat(target_path: VirtAddr, fd: i32, link_path: VirtAddr) -> EResult<()> {
    if target_path == VirtAddr::null() || link_path == VirtAddr::null() {
        return Err(Errno::EINVAL);
    }

    let target_path_buf = UserCStr::new(target_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;
    let link_path_buf = UserCStr::new(link_path)
        .as_vec(PATH_MAX)
        .ok_or(Errno::EFAULT)?;

    if target_path_buf.is_empty() {
        return Err(Errno::ENOENT);
    }

    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();
    let parent = if fd == AT_FDCWD as _ {
        proc.working_dir.lock().clone()
    } else {
        files
            .get_fd(fd)
            .ok_or(Errno::EBADF)?
            .file
            .path
            .as_ref()
            .ok_or(Errno::ENOTDIR)?
            .clone()
    };

    let root = proc.root_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    drop(files);

    vfs::symlink(root, parent, &link_path_buf, &target_path_buf, &identity)
}

#[wrap_syscall]
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

#[wrap_syscall]
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

#[wrap_syscall]
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

    *proc.root_dir.lock() = node;
    Ok(0)
}

#[wrap_syscall]
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

fn resolve_epoll(fd: i32) -> EResult<Arc<File>> {
    let proc = Scheduler::get_current().get_process();
    let file = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?.file;
    if Arc::downcast::<EpollFile>(file.ops.clone()).is_err() {
        return Err(Errno::EINVAL);
    }
    Ok(file)
}

fn epoll_of(file: &File) -> Arc<EpollFile> {
    Arc::downcast(file.ops.clone()).expect("file was previously verified as an epoll")
}

const fn poll_flags_from_epoll(events: u32) -> PollFlags {
    // Drop ET/ONESHOT/EXCLUSIVE/WAKEUP metadata bits and EPOLLMSG, which collides
    // with POLLNVAL in the PollFlags encoding and has no poll equivalent.
    let bits = (events & 0x07FF) & !EPOLLMSG;
    PollFlags::from_bits_truncate(bits as i16)
}

const fn epoll_bits_from_poll(revents: PollFlags) -> u32 {
    (revents.bits() as u16) as u32
}

#[wrap_syscall]
pub fn epoll_create(flags: i32) -> EResult<i32> {
    if flags & !(O_CLOEXEC as i32) != 0 {
        return Err(Errno::EINVAL);
    }

    let ops: Arc<dyn FileOps> = Arc::new(EpollFile::new());
    let file = File::open_disconnected(ops, OpenFlags::ReadWrite)?;

    let proc = Scheduler::get_current().get_process();
    let mut open_files = proc.open_files.lock();
    let fd = open_files
        .open_file(
            FileDescription {
                file,
                close_on_exec: AtomicBool::new(flags & O_CLOEXEC as i32 != 0),
            },
            0,
        )
        .ok_or(Errno::EMFILE)?;

    Ok(fd)
}

#[wrap_syscall]
pub fn epoll_ctl(epfd: i32, op: i32, fd: i32, event_ptr: VirtAddr) -> EResult<()> {
    if epfd == fd {
        return Err(Errno::EINVAL);
    }

    let epfile = resolve_epoll(epfd)?;
    let epoll = epoll_of(&epfile);

    let target = {
        let proc = Scheduler::get_current().get_process();
        let open_files = proc.open_files.lock();
        open_files.get_fd(fd).ok_or(Errno::EBADF)?.file
    };

    match op {
        EPOLL_CTL_ADD => {
            let ev = UserPtr::<epoll_event>::new(event_ptr)
                .read()
                .ok_or(Errno::EFAULT)?;
            if ev.events & EPOLLEXCLUSIVE != 0 {
                warn!("epoll_ctl: EPOLLEXCLUSIVE is not supported");
            }
            if ev.events & EPOLLET != 0 {
                warn!("epoll_ctl: EPOLLET requested, treating as level-triggered");
            }
            epoll.add(fd, target, ev.events, unsafe { ev.data.num_u64 })
        }
        EPOLL_CTL_MOD => {
            let ev = UserPtr::<epoll_event>::new(event_ptr)
                .read()
                .ok_or(Errno::EFAULT)?;
            epoll.modify(fd, ev.events, unsafe { ev.data.num_u64 })
        }
        EPOLL_CTL_DEL => epoll.remove(fd),
        _ => Err(Errno::EINVAL),
    }
}

#[wrap_syscall]
pub fn epoll_pwait(
    epfd: i32,
    events_ptr: VirtAddr,
    maxevents: i32,
    timeout_ms: i32,
    sigmask_ptr: VirtAddr,
) -> EResult<usize> {
    if maxevents <= 0 {
        return Err(Errno::EINVAL);
    }
    let _ = sigmask_ptr; // TODO: apply signal mask during wait

    let epfile = resolve_epoll(epfd)?;
    let epoll = epoll_of(&epfile);
    let maxevents = maxevents as usize;

    let is_nonblocking = timeout_ms == 0;
    let timeout_guard = if timeout_ms > 0 {
        let deadline = clock::get_elapsed().saturating_add((timeout_ms as usize) * 1_000_000);
        Some(clock::timeout_at(deadline))
    } else {
        None
    };

    let registrations = epoll.snapshot();
    let mut wait_files: Vec<Arc<File>> = Vec::new();
    if !is_nonblocking {
        for (_, _, _, file) in &registrations {
            if let Ok(inner) = Arc::downcast::<EpollFile>(file.ops.clone()) {
                inner.get_children(&mut wait_files);
            } else {
                wait_files.push(file.clone());
            }
        }
    }
    let guards: Vec<_> = if is_nonblocking {
        Vec::new()
    } else {
        let mut guards = Vec::new();
        for file in &wait_files {
            // Use Read mask so we wait on any readability.
            for ev in file.ops.poll_events(file, PollFlags::Read).iter() {
                guards.push(ev.guard());
            }
        }
        guards
    };

    let mut out: Vec<epoll_event> = Vec::with_capacity(maxevents);
    let mut oneshot_fired: Vec<i32> = Vec::new();

    loop {
        out.clear();
        oneshot_fired.clear();

        for (fd, events, data, file) in &registrations {
            if out.len() >= maxevents {
                break;
            }
            let mask = poll_flags_from_epoll(*events);
            let revents = match file.ops.poll(file, mask) {
                Ok(r) => r,
                Err(_) => PollFlags::Err,
            };
            let reported = epoll_bits_from_poll(revents) & *events;
            if reported != 0 {
                out.push(epoll_event {
                    events: reported,
                    data: epoll_data { num_u64: *data },
                });
                if *events & EPOLLONESHOT != 0 {
                    oneshot_fired.push(*fd);
                }
            }
        }

        if !out.is_empty() || is_nonblocking {
            break;
        }
        if timeout_guard.as_ref().is_some_and(|g| g.expired()) {
            break;
        }
        // No way to be woken: empty interest list and indefinite wait would hang.
        if guards.is_empty() && timeout_guard.is_none() {
            break;
        }

        if let Some(g) = guards.first() {
            g.wait();
        } else {
            CpuData::get().scheduler.do_yield();
        }

        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::EINTR);
        }
    }

    for fd in oneshot_fired {
        epoll.disarm_oneshot(fd);
    }

    let mut writer = UserPtr::<epoll_event>::new(events_ptr);
    writer.write_slice(&out).ok_or(Errno::EFAULT)?;
    Ok(out.len())
}

#[wrap_syscall]
pub fn timerfd_create(clockid: i32, flags: i32) -> EResult<i32> {
    // TODO: We don't currently distinguish clocks; accept any sensible id.
    let _ = clockid;

    let allowed = (TFD_CLOEXEC | TFD_NONBLOCK) as i32;
    if flags & !allowed != 0 {
        return Err(Errno::EINVAL);
    }

    let timer = TimerfdFile::new();
    let ops: Arc<dyn FileOps> = timer;

    let mut open_flags = OpenFlags::ReadWrite;
    if flags & TFD_NONBLOCK as i32 != 0 {
        open_flags |= OpenFlags::NonBlocking;
    }
    let file = File::open_disconnected(ops, open_flags)?;

    let proc = Scheduler::get_current().get_process();
    let mut open_files = proc.open_files.lock();
    open_files
        .open_file(
            FileDescription {
                file,
                close_on_exec: AtomicBool::new(flags & TFD_CLOEXEC as i32 != 0),
            },
            0,
        )
        .ok_or(Errno::EMFILE)
}

#[wrap_syscall]
pub fn timerfd_gettime(fd: i32, value: UserPtr<itimerspec>) -> EResult<i32> {
    let file = {
        let proc = Scheduler::get_current().get_process();
        let proc_inner = proc.open_files.lock();
        proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file
    };
    let timer = Arc::downcast::<TimerfdFile>(file.ops.clone()).map_err(|_| Errno::EINVAL)?;

    let mut value = value;
    let now = clock::get_elapsed();
    value.write(timer.gettime(now)).ok_or(Errno::EFAULT)?;
    Ok(0)
}

#[wrap_syscall]
pub fn timerfd_settime(
    fd: i32,
    flags: i32,
    new_value: UserPtr<itimerspec>,
    old_value: UserPtr<itimerspec>,
) -> EResult<i32> {
    if flags & !TFD_TIMER_ABSTIME != 0 {
        return Err(Errno::EINVAL);
    }

    let file = {
        let proc = Scheduler::get_current().get_process();
        let proc_inner = proc.open_files.lock();
        proc_inner.get_fd(fd).ok_or(Errno::EBADF)?.file
    };
    let timer = Arc::downcast::<TimerfdFile>(file.ops.clone()).map_err(|_| Errno::EINVAL)?;

    let new = new_value.read().ok_or(Errno::EFAULT)?;
    let interval_ns = timerfd::timespec_to_ns(new.it_interval)?;
    let initial_ns = timerfd::timespec_to_ns(new.it_value)?;

    let now = clock::get_elapsed();
    let initial_deadline = if initial_ns == 0 {
        // Disarm.
        None
    } else if flags & TFD_TIMER_ABSTIME != 0 {
        Some(initial_ns)
    } else {
        Some(now.checked_add(initial_ns).ok_or(Errno::EINVAL)?)
    };

    let old = timer.settime(now, initial_deadline, interval_ns);

    if !old_value.is_null() {
        let mut old_value = old_value;
        old_value.write(old).ok_or(Errno::EFAULT)?;
    }

    Ok(0)
}

#[wrap_syscall]
pub fn signalfd_create(mask: UserPtr<sigset_t>, flags: i32) -> EResult<i32> {
    let allowed = (SFD_CLOEXEC | SFD_NONBLOCK) as i32;
    if flags & !allowed != 0 {
        return Err(Errno::EINVAL);
    }

    let set = if !mask.is_null() {
        let raw_mask = mask.read().ok_or(Errno::EFAULT)?;
        SignalSet::from_raw(raw_mask)
    } else {
        SignalSet::from_raw(0)
    };

    let proc = Scheduler::get_current().get_process();
    let ops: Arc<dyn FileOps> = Arc::new(SignalfdFile::new(proc.clone(), set));

    let mut open_flags = OpenFlags::ReadWrite;
    if flags & SFD_NONBLOCK as i32 != 0 {
        open_flags |= OpenFlags::NonBlocking;
    }
    let file = File::open_disconnected(ops, open_flags)?;

    let mut open_files = proc.open_files.lock();
    open_files
        .open_file(
            FileDescription {
                file,
                close_on_exec: AtomicBool::new(flags & SFD_CLOEXEC as i32 != 0),
            },
            0,
        )
        .ok_or(Errno::EMFILE)
}
