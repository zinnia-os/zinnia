use crate::{
    device::net::{Socket, SocketFlags, SocketOps, local::LocalSocket},
    memory::{IovecIter, UserPtr, VirtAddr},
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::socket::*,
    vfs::{
        File,
        file::{FileDescription, FileOps, OpenFlags},
    },
    wrap_syscall,
};
use alloc::{sync::Arc, vec::Vec};
use core::{mem::offset_of, sync::atomic::AtomicBool};

/// Extract an `Arc<Socket>` from a file descriptor.
fn get_socket(fd: i32) -> EResult<Arc<Socket>> {
    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();
    let desc = files.get_fd(fd).ok_or(Errno::EBADF)?;

    // Downcast Arc<dyn FileOps> to Arc<Socket>.
    Arc::downcast(desc.file.ops.clone()).map_err(|_| Errno::ENOTSOCK)
}

/// Check if the file for an fd has NonBlocking set.
fn is_fd_nonblocking(fd: i32) -> EResult<bool> {
    let proc = Scheduler::get_current().get_process();
    let files = proc.open_files.lock();
    let desc = files.get_fd(fd).ok_or(Errno::EBADF)?;
    Ok(desc.file.flags.lock().contains(OpenFlags::NonBlocking))
}

/// Read raw sockaddr bytes from user memory.
fn read_sockaddr(ptr: UserPtr<u8>, addr_length: usize) -> EResult<Vec<u8>> {
    if ptr.is_null() || addr_length == 0 {
        return Err(Errno::EINVAL);
    }
    let mut buf = vec![0u8; addr_length];
    ptr.read_slice(&mut buf).ok_or(Errno::EFAULT)?;
    Ok(buf)
}

/// Write a sockaddr to user memory, updating the length pointer.
fn write_sockaddr(
    mut addr_ptr: UserPtr<u8>,
    mut len_ptr: UserPtr<socklen_t>,
    addr: &[u8],
) -> EResult<()> {
    if addr_ptr.is_null() || len_ptr.is_null() {
        return Ok(()); // Optional output pointers.
    }

    let max_len = len_ptr.read().ok_or(Errno::EFAULT)? as usize;
    let copy_len = max_len.min(addr.len());

    if copy_len > 0 {
        addr_ptr
            .write_slice(&addr[..copy_len])
            .ok_or(Errno::EFAULT)?;
    }

    // Write back the actual address length.
    len_ptr
        .write(addr.len() as socklen_t)
        .ok_or(Errno::EFAULT)?;
    Ok(())
}

#[wrap_syscall]
pub fn socket(family: i32, socket_type: i32, _protocol: i32) -> EResult<usize> {
    let flags = SocketFlags::from_bits_truncate(socket_type as u32);
    let sock_type = socket_type as u32 & !(SOCK_NONBLOCK | SOCK_CLOEXEC | SOCK_CLOFORK);

    let ops: Arc<dyn SocketOps> = match family as u32 {
        AF_UNIX => LocalSocket::new(sock_type)?,
        _ => return Err(Errno::EAFNOSUPPORT),
    };

    let socket = Socket::new(family as u32, sock_type, ops)?;

    let mut open_flags = OpenFlags::ReadWrite;
    if flags.contains(SocketFlags::NonBlocking) {
        open_flags |= OpenFlags::NonBlocking;
    }

    let file = File::open_disconnected(socket as Arc<dyn FileOps>, open_flags)?;

    let proc = Scheduler::get_current().get_process();
    let mut files = proc.open_files.lock();
    let fd = files
        .open_file(
            FileDescription {
                file,
                close_on_exec: AtomicBool::new(flags.contains(SocketFlags::CloseOnExec)),
            },
            0,
        )
        .ok_or(Errno::EMFILE)?;

    Ok(fd as usize)
}

#[wrap_syscall]
pub fn socketpair(domain: i32, type_and_flags: u32, _protocol: i32) -> EResult<usize> {
    let flags = SocketFlags::from_bits_truncate(type_and_flags);
    let sock_type = type_and_flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC | SOCK_CLOFORK);

    let (sa, sb) = match domain as u32 {
        AF_UNIX => LocalSocket::new_pair(sock_type)?,
        _ => return Err(Errno::EAFNOSUPPORT),
    };

    let mut open_flags = OpenFlags::ReadWrite;
    if flags.contains(SocketFlags::NonBlocking) {
        open_flags |= OpenFlags::NonBlocking;
    }
    let cloexec = flags.contains(SocketFlags::CloseOnExec);

    let file_a = File::open_disconnected(sa as Arc<dyn FileOps>, open_flags)?;
    let file_b = File::open_disconnected(sb as Arc<dyn FileOps>, open_flags)?;

    let proc = Scheduler::get_current().get_process();
    let mut files = proc.open_files.lock();

    let fd0 = files
        .open_file(
            FileDescription {
                file: file_a,
                close_on_exec: AtomicBool::new(cloexec),
            },
            0,
        )
        .ok_or(Errno::EMFILE)?;

    let fd1 = files
        .open_file(
            FileDescription {
                file: file_b,
                close_on_exec: AtomicBool::new(cloexec),
            },
            0,
        )
        .ok_or(Errno::EMFILE)?;

    Ok((fd0 as usize) | ((fd1 as usize) << 32))
}

#[wrap_syscall]
pub fn bind(fd: i32, addr_ptr: VirtAddr, addr_length: usize) -> EResult<()> {
    let addr_ptr = UserPtr::<u8>::new(addr_ptr);
    let addr = read_sockaddr(addr_ptr, addr_length)?;
    let socket = get_socket(fd)?;
    socket.ops.bind(&addr, &socket)
}

#[wrap_syscall]
pub fn listen(fd: i32, backlog: i32) -> EResult<()> {
    let socket = get_socket(fd)?;
    socket.ops.listen(backlog)
}

#[wrap_syscall]
pub fn accept(
    fd: i32,
    newfd: VirtAddr,
    addr_ptr: VirtAddr,
    addr_length: VirtAddr,
    flags: usize,
) -> EResult<()> {
    let mut newfd_ptr = UserPtr::<i32>::new(newfd);
    let addr_ptr = UserPtr::<u8>::new(addr_ptr);
    let addr_len_ptr = UserPtr::<socklen_t>::new(addr_length);

    let socket = get_socket(fd)?;
    let nonblocking = is_fd_nonblocking(fd)?;
    let accept_flags = SocketFlags::from_bits_truncate(flags as u32);

    let new_socket = socket.ops.accept(nonblocking)?;

    // Get the peer address if requested.
    if !addr_ptr.is_null() && !addr_len_ptr.is_null() {
        let mut addr_buf = [0u8; 128];
        let addr_len = new_socket.ops.getpeername(&mut addr_buf)?;
        write_sockaddr(addr_ptr, addr_len_ptr, &addr_buf[..addr_len])?;
    }

    // Open the new socket as a file.
    let mut open_flags = OpenFlags::ReadWrite;
    if accept_flags.contains(SocketFlags::NonBlocking) {
        open_flags |= OpenFlags::NonBlocking;
    }

    let file = File::open_disconnected(new_socket as Arc<dyn FileOps>, open_flags)?;

    let proc = Scheduler::get_current().get_process();
    let mut files = proc.open_files.lock();
    let new_fd = files
        .open_file(
            FileDescription {
                file,
                close_on_exec: AtomicBool::new(accept_flags.contains(SocketFlags::CloseOnExec)),
            },
            0,
        )
        .ok_or(Errno::EMFILE)?;

    newfd_ptr.write(new_fd).ok_or(Errno::EFAULT)
}

#[wrap_syscall]
pub fn connect(fd: i32, addr_ptr: VirtAddr, addr_length: usize) -> EResult<()> {
    let addr_ptr = UserPtr::<u8>::new(addr_ptr);
    let addr = read_sockaddr(addr_ptr, addr_length)?;
    let socket = get_socket(fd)?;
    let nonblocking = is_fd_nonblocking(fd)?;
    socket.ops.connect(&addr, nonblocking)
}

#[wrap_syscall]
pub fn sendmsg(fd: i32, hdr: VirtAddr, flags: i32) -> EResult<usize> {
    let hdr_ptr = UserPtr::<msghdr>::new(hdr);

    let socket = get_socket(fd)?;
    let nonblocking = is_fd_nonblocking(fd)?;

    let msg = hdr_ptr.read().ok_or(Errno::EFAULT)?;

    let iovcnt = msg.msg_iovlen as usize;
    let mut iovecs = Vec::with_capacity(iovcnt);
    for i in 0..iovcnt {
        iovecs.push(msg.msg_iov.offset(i).read().ok_or(Errno::EFAULT)?);
    }
    let mut iter = IovecIter::new(&iovecs)?;

    let ctrl_len = msg.msg_controllen as usize;
    let ctrl_buf = if ctrl_len > 0 && !msg.msg_control.is_null() {
        let ptr = UserPtr::<u8>::new(msg.msg_control.addr());
        let mut v = vec![0u8; ctrl_len];
        ptr.read_slice(&mut v).ok_or(Errno::EFAULT)?;
        v
    } else {
        Vec::new()
    };

    let sent = socket
        .ops
        .sendmsg(&mut iter, &ctrl_buf, flags as u32, nonblocking)?;

    Ok(sent as usize)
}

#[wrap_syscall]
pub fn recvmsg(fd: i32, hdr: VirtAddr, flags: i32) -> EResult<usize> {
    let hdr_ptr = UserPtr::<msghdr>::new(hdr);

    let socket = get_socket(fd)?;
    let nonblocking = is_fd_nonblocking(fd)?;

    let msg = hdr_ptr.read().ok_or(Errno::EFAULT)?;

    let iovcnt = msg.msg_iovlen as usize;
    let mut iovecs = Vec::with_capacity(iovcnt);
    for i in 0..iovcnt {
        iovecs.push(msg.msg_iov.offset(i).read().ok_or(Errno::EFAULT)?);
    }
    let mut iter = IovecIter::new(&iovecs)?;

    let ctrl_cap = msg.msg_controllen as usize;
    let mut ctrl_buf = vec![0u8; ctrl_cap];

    let (received, ctrl_written, out_flags) =
        socket
            .ops
            .recvmsg(&mut iter, &mut ctrl_buf, flags as u32, nonblocking)?;

    if ctrl_written > 0 && !msg.msg_control.is_null() {
        let mut ptr = UserPtr::<u8>::new(msg.msg_control.addr());
        ptr.write_slice(&ctrl_buf[..ctrl_written])
            .ok_or(Errno::EFAULT)?;
    }

    // Write back msg_controllen and msg_flags so userspace doesn't parse
    // uninitialized control memory.
    let mut controllen_ptr = UserPtr::<socklen_t>::new(
        hdr_ptr.addr() + VirtAddr::new(offset_of!(msghdr, msg_controllen)),
    );
    controllen_ptr
        .write(ctrl_written as socklen_t)
        .ok_or(Errno::EFAULT)?;

    let mut flags_ptr =
        UserPtr::<i32>::new(hdr_ptr.addr() + VirtAddr::new(offset_of!(msghdr, msg_flags)));
    flags_ptr.write(out_flags as i32).ok_or(Errno::EFAULT)?;

    Ok(received as usize)
}

#[wrap_syscall]
pub fn shutdown(fd: i32, how: i32) -> EResult<()> {
    let socket = get_socket(fd)?;
    socket.ops.shutdown(how as u32)
}

#[wrap_syscall]
pub fn getsockopt(
    fd: i32,
    layer: i32,
    number: i32,
    buffer: VirtAddr,
    size: VirtAddr,
) -> EResult<()> {
    let mut buf_ptr = UserPtr::<u8>::new(buffer);
    let mut size_ptr = UserPtr::<socklen_t>::new(size);

    let socket = get_socket(fd)?;

    let max_len = size_ptr.read().ok_or(Errno::EFAULT)? as usize;
    let mut buf = vec![0u8; max_len];

    let actual_len = socket.ops.getsockopt(layer, number, &mut buf)?;

    let copy_len = max_len.min(actual_len);
    if copy_len > 0 {
        buf_ptr.write_slice(&buf[..copy_len]).ok_or(Errno::EFAULT)?;
    }
    size_ptr
        .write(actual_len as socklen_t)
        .ok_or(Errno::EFAULT)?;

    Ok(())
}

#[wrap_syscall]
pub fn setsockopt(fd: i32, layer: i32, number: i32, buffer: VirtAddr, size: usize) -> EResult<()> {
    let buf_ptr = UserPtr::<u8>::new(buffer);

    let socket = get_socket(fd)?;

    let mut buf = vec![0u8; size];
    if size > 0 {
        buf_ptr.read_slice(&mut buf).ok_or(Errno::EFAULT)?;
    }

    socket.ops.setsockopt(layer, number, &buf)
}

#[wrap_syscall]
pub fn getsockname(fd: i32, addr_ptr: VirtAddr, max_addr_len: VirtAddr) -> EResult<()> {
    let addr_ptr = UserPtr::<u8>::new(addr_ptr);
    let len_ptr = UserPtr::<socklen_t>::new(max_addr_len);

    let socket = get_socket(fd)?;
    let mut buf = [0u8; 128];
    let len = socket.ops.getsockname(&mut buf)?;
    write_sockaddr(addr_ptr, len_ptr, &buf[..len])
}

#[wrap_syscall]
pub fn getpeername(fd: i32, addr_ptr: VirtAddr, max_addr_len: VirtAddr) -> EResult<()> {
    let addr_ptr = UserPtr::<u8>::new(addr_ptr);
    let len_ptr = UserPtr::<socklen_t>::new(max_addr_len);

    let socket = get_socket(fd)?;
    let mut buf = [0u8; 128];
    let len = socket.ops.getpeername(&mut buf)?;
    write_sockaddr(addr_ptr, len_ptr, &buf[..len])
}
