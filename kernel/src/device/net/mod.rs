use crate::{
    memory::{IovecIter, VirtAddr},
    posix::errno::{EResult, Errno},
    uapi,
    vfs::{
        File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
    },
};
use alloc::sync::Arc;
use bitflags::bitflags;
use core::any::Any;

pub mod local;

pub struct Socket {
    pub ops: Arc<dyn SocketOps>,
    pub socket_type: u32,
    pub family: u32,
}

impl Socket {
    pub fn new(family: u32, socket_type: u32, ops: Arc<dyn SocketOps>) -> EResult<Arc<Self>> {
        Ok(Arc::try_new(Self {
            ops,
            family,
            socket_type,
        })?)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct SocketFlags: u32 {
        const CloseOnExec = uapi::socket::SOCK_CLOEXEC;
        const CloseOnFork = uapi::socket::SOCK_CLOFORK;
        const NonBlocking = uapi::socket::SOCK_NONBLOCK;
    }


    #[derive(Debug, Default, Clone, Copy)]
    pub struct ShutdownFlags: u32 {
        const Read = uapi::socket::SHUT_RD;
        const Write = uapi::socket::SHUT_WR;
    }
}

pub trait SocketOps: Send + Sync + Any {
    /// Bind socket to an address.
    fn bind(&self, addr: &[u8], socket: &Arc<Socket>) -> EResult<()>;

    /// Start listening for connections.
    fn listen(&self, backlog: i32) -> EResult<()>;

    /// Accept an incoming connection. Returns a new Socket for the server side.
    fn accept(&self, nonblocking: bool) -> EResult<Arc<Socket>>;

    /// Connect to a remote/local address.
    fn connect(&self, addr: &[u8], nonblocking: bool) -> EResult<()>;

    /// Send data.
    fn send(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize>;

    /// Receive data.
    fn recv(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize>;

    /// Send data with optional ancillary (control) data.
    ///
    /// `control` is the raw cmsg blob exactly as the caller wrote it.
    /// Default implementation discards `control` and forwards to `send`.
    fn sendmsg(
        &self,
        buf: &mut IovecIter,
        control: &[u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<isize> {
        let _ = control;
        self.send(buf, flags, nonblocking)
    }

    /// Receive data with optional ancillary (control) data.
    ///
    /// Writes up to `control.len()` bytes of cmsg output into `control`.
    /// Returns `(data_bytes, control_bytes, out_msg_flags)`. `out_msg_flags`
    /// may include `MSG_CTRUNC` if ancillary data did not fit.
    /// Default implementation writes no control and forwards to `recv`.
    fn recvmsg(
        &self,
        buf: &mut IovecIter,
        control: &mut [u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<(isize, usize, u32)> {
        let _ = control;
        let n = self.recv(buf, flags, nonblocking)?;
        Ok((n, 0, 0))
    }

    /// Shutdown read/write/both.
    fn shutdown(&self, how: u32) -> EResult<()>;

    /// Get local address.
    fn getsockname(&self, buf: &mut [u8]) -> EResult<usize>;

    /// Get peer address.
    fn getpeername(&self, buf: &mut [u8]) -> EResult<usize>;

    /// Get socket option.
    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize>;

    /// Set socket option.
    fn setsockopt(&self, level: i32, optname: i32, buf: &[u8]) -> EResult<()>;

    /// Poll for readiness.
    fn poll(&self, mask: PollFlags) -> EResult<PollFlags>;

    /// Get the events relevant to the requested poll mask.
    fn poll_events(&self, mask: PollFlags) -> PollEventSet<'_>;

    /// Called when last file reference is dropped.
    fn release(&self) -> EResult<()> {
        Ok(())
    }
}

impl FileOps for Socket {
    fn read(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        let nb = file.flags.lock().contains(OpenFlags::NonBlocking);
        self.ops.recv(buf, 0, nb)
    }

    fn write(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        let nb = file.flags.lock().contains(OpenFlags::NonBlocking);
        self.ops.send(buf, 0, nb)
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        self.ops.poll(mask)
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        self.ops.poll_events(mask)
    }

    fn release(&self, _file: &File) -> EResult<()> {
        self.ops.release()
    }

    fn ioctl(&self, _file: &File, request: usize, argp: VirtAddr) -> EResult<usize> {
        let _ = (request, argp);
        Err(Errno::ENOTTY)
    }
}
