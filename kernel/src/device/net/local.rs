use alloc::sync::Weak;
use alloc::{collections::VecDeque, sync::Arc, vec, vec::Vec};
use core::cmp::min;

use crate::device::net::ShutdownFlags;
use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::socket::*,
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{
        self, PathNode,
        cache::LookupFlags,
        file::{PollEventSet, PollFlags},
        inode::{Device, Mode, NodeOps},
    },
};

use super::{Socket, SocketOps};

const BUFFER_SIZE: usize = 0x1000;

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Unconnected,
    Bound,
    Listening,
    Connected,
}

struct LocalInner {
    state: State,
    sock_type: u32,
    bound_addr: Option<Vec<u8>>,
    peer: Option<Arc<LocalSocket>>,
    self_ref: Weak<LocalSocket>,
    recv_buf: RingBuffer,
    backlog: VecDeque<Arc<Socket>>,
    backlog_limit: usize,
    shutdown: ShutdownFlags,
    peer_closed: bool,
}

pub struct LocalSocket {
    inner: SpinMutex<LocalInner>,
    rd_event: Event,
    wr_event: Event,
    accept_event: Event,
}

fn make_socket(state: State, sock_type: u32) -> EResult<Arc<LocalSocket>> {
    let this = Arc::try_new(LocalSocket {
        inner: SpinMutex::new(LocalInner {
            state,
            sock_type,
            bound_addr: None,
            peer: None,
            self_ref: Weak::new(),
            recv_buf: RingBuffer::new(BUFFER_SIZE),
            backlog: VecDeque::new(),
            backlog_limit: 0,
            shutdown: ShutdownFlags::empty(),
            peer_closed: false,
        }),
        rd_event: Event::new(),
        wr_event: Event::new(),
        accept_event: Event::new(),
    })?;
    this.inner.lock().self_ref = Arc::downgrade(&this);
    Ok(this)
}

impl LocalSocket {
    pub fn new(sock_type: u32) -> EResult<Arc<Self>> {
        make_socket(State::Unconnected, sock_type)
    }

    pub fn new_pair(sock_type: u32) -> EResult<(Arc<Socket>, Arc<Socket>)> {
        let a = make_socket(State::Connected, sock_type)?;
        let b = make_socket(State::Connected, sock_type)?;

        a.inner.lock().peer = Some(b.clone());
        b.inner.lock().peer = Some(a.clone());

        let sa = Socket::new(AF_UNIX, sock_type, a)?;
        let sb = Socket::new(AF_UNIX, sock_type, b)?;
        Ok((sa, sb))
    }

    fn parse_path(addr: &[u8]) -> EResult<&[u8]> {
        if addr.len() < size_of::<sa_family_t>() + 1 {
            return Err(Errno::EINVAL);
        }
        let path_bytes = &addr[size_of::<sa_family_t>()..];
        let path_len = path_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_bytes.len());
        if path_len == 0 {
            return Err(Errno::EINVAL);
        }
        Ok(&path_bytes[..path_len])
    }

    fn build_sockaddr(path: &[u8]) -> Vec<u8> {
        let family_size = size_of::<sa_family_t>();
        let mut addr = vec![0u8; family_size + path.len() + 1];
        let family = AF_UNIX as sa_family_t;
        addr[..family_size].copy_from_slice(&family.to_ne_bytes());
        addr[family_size..family_size + path.len()].copy_from_slice(path);
        addr
    }

    fn write_addr(addr: &Option<Vec<u8>>, buf: &mut [u8]) -> usize {
        match addr {
            Some(a) => {
                let len = min(a.len(), buf.len());
                buf[..len].copy_from_slice(&a[..len]);
                a.len()
            }
            None => {
                let family = AF_UNIX as sa_family_t;
                let bytes = family.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                size_of::<sa_family_t>()
            }
        }
    }
}

impl SocketOps for LocalSocket {
    fn bind(&self, addr: &[u8], socket: &Arc<Socket>) -> EResult<()> {
        let path = Self::parse_path(addr)?;

        {
            let mut inner = self.inner.lock();
            if inner.state != State::Unconnected {
                return Err(Errno::EINVAL);
            }
            inner.bound_addr = Some(Self::build_sockaddr(path));
            inner.state = State::Bound;
        }

        let proc = Scheduler::get_current().get_process();
        let root = proc.root_dir.lock().clone();
        let cwd = proc.working_dir.lock().clone();
        let identity = proc.identity.lock().clone();
        let mode = Mode::from_bits_truncate(0o777);
        vfs::mknod(
            root,
            cwd,
            path,
            mode,
            Some(Device::Socket(socket.clone())),
            &identity,
        )?;

        Ok(())
    }

    fn listen(&self, backlog: i32) -> EResult<()> {
        let mut inner = self.inner.lock();
        match inner.state {
            State::Bound | State::Listening => {
                inner.state = State::Listening;
                inner.backlog_limit = (backlog as usize).max(1);
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn accept(&self, nonblocking: bool) -> EResult<Arc<Socket>> {
        let guard = self.accept_event.guard();
        loop {
            {
                let mut inner = self.inner.lock();
                if inner.state != State::Listening {
                    return Err(Errno::EINVAL);
                }
                if let Some(sock) = inner.backlog.pop_front() {
                    return Ok(sock);
                }
            }
            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if crate::sched::Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn connect(&self, addr: &[u8], _nonblocking: bool) -> EResult<()> {
        let path = Self::parse_path(addr)?;

        let (sock_type, self_arc) = {
            let inner = self.inner.lock();
            if inner.state == State::Connected {
                return Err(Errno::EISCONN);
            }
            if inner.state == State::Listening {
                return Err(Errno::EINVAL);
            }
            let self_arc = inner.self_ref.upgrade().ok_or(Errno::EBADF)?;
            (inner.sock_type, self_arc)
        };

        let proc = Scheduler::get_current().get_process();
        let root = proc.root_dir.lock().clone();
        let cwd = proc.working_dir.lock().clone();
        let identity = proc.identity.lock().clone();

        let target = PathNode::lookup(
            root,
            cwd,
            path,
            &identity,
            LookupFlags::MustExist | LookupFlags::FollowSymlinks,
        )?;

        let inode = target.entry.get_inode().ok_or(Errno::ENOENT)?;
        let listener_socket = match &inode.node_ops {
            NodeOps::Socket(s) => s.clone(),
            _ => return Err(Errno::ECONNREFUSED),
        };

        let listener: Arc<LocalSocket> =
            Arc::downcast(listener_socket.ops.clone()).map_err(|_| Errno::ECONNREFUSED)?;

        let server_end = make_socket(State::Connected, sock_type)?;

        {
            let mut listener_inner = listener.inner.lock();
            if listener_inner.state != State::Listening {
                return Err(Errno::ECONNREFUSED);
            }
            if listener_inner.backlog.len() >= listener_inner.backlog_limit {
                return Err(Errno::ECONNREFUSED);
            }

            server_end.inner.lock().peer = Some(self_arc);

            let server_socket = Socket::new(AF_UNIX, sock_type, server_end.clone())?;
            listener_inner.backlog.push_back(server_socket);
        }

        {
            let mut self_inner = self.inner.lock();
            self_inner.peer = Some(server_end);
            self_inner.state = State::Connected;
        }

        listener.accept_event.wake_one();
        listener.rd_event.wake_one();

        Ok(())
    }

    fn send(&self, buf: &mut IovecIter, _flags: u32, nonblocking: bool) -> EResult<isize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let peer = {
            let inner = self.inner.lock();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            if inner.shutdown.contains(ShutdownFlags::Write) {
                return Err(Errno::EPIPE);
            }
            inner.peer.clone().ok_or(Errno::ENOTCONN)?
        };

        let guard = peer.wr_event.guard();
        loop {
            {
                let mut peer_inner = peer.inner.lock();
                if peer_inner.peer_closed || peer_inner.shutdown.contains(ShutdownFlags::Read) {
                    return Err(Errno::EPIPE);
                }

                let mut v = vec![0u8; buf.len()];
                buf.copy_to_slice(&mut v)?;
                let written = peer_inner.recv_buf.write(&v);
                if written > 0 {
                    peer.rd_event.wake_one();
                    return Ok(written as isize);
                }
            }

            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if crate::sched::Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn recv(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let peek = flags & MSG_PEEK != 0;

        let guard = self.rd_event.guard();
        loop {
            {
                let mut inner = self.inner.lock();
                if inner.state != State::Connected {
                    return Err(Errno::ENOTCONN);
                }

                let mut v = vec![0u8; buf.len()];
                let len = if peek {
                    inner.recv_buf.peek(&mut v)
                } else {
                    inner.recv_buf.read(&mut v)
                };

                if len > 0 {
                    buf.copy_from_slice(&v[..len])?;
                    if !peek {
                        if let Some(peer) = &inner.peer {
                            peer.wr_event.wake_one();
                        }
                    }
                    return Ok(len as isize);
                }

                if inner.shutdown.contains(ShutdownFlags::Read) || inner.peer_closed {
                    return Ok(0);
                }
            }

            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if crate::sched::Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn shutdown(&self, how: u32) -> EResult<()> {
        let flags = ShutdownFlags::from_bits_truncate(how);
        let peer = {
            let mut inner = self.inner.lock();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            inner.shutdown |= flags;
            inner.peer.clone()
        };

        if flags.contains(ShutdownFlags::Read) {
            self.rd_event.wake_all();
        }
        if flags.contains(ShutdownFlags::Write) {
            if let Some(peer) = &peer {
                peer.rd_event.wake_all();
            }
            self.wr_event.wake_all();
        }
        Ok(())
    }

    fn getsockname(&self, buf: &mut [u8]) -> EResult<usize> {
        let inner = self.inner.lock();
        Ok(Self::write_addr(&inner.bound_addr, buf))
    }

    fn getpeername(&self, buf: &mut [u8]) -> EResult<usize> {
        let inner = self.inner.lock();
        if inner.state != State::Connected {
            return Err(Errno::ENOTCONN);
        }
        let peer_addr = inner
            .peer
            .as_ref()
            .and_then(|p| p.inner.lock().bound_addr.clone());
        Ok(Self::write_addr(&peer_addr, buf))
    }

    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        match optname as u32 {
            SO_TYPE => {
                let val = self.inner.lock().sock_type as i32;
                let bytes = val.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_ERROR => {
                let bytes = 0i32.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_SNDBUF | SO_RCVBUF => {
                let val = BUFFER_SIZE as i32;
                let bytes = val.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_ACCEPTCONN => {
                let val = (self.inner.lock().state == State::Listening) as i32;
                let bytes = val.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    fn setsockopt(&self, level: i32, optname: i32, _buf: &[u8]) -> EResult<()> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        match optname as u32 {
            SO_SNDBUF | SO_RCVBUF | SO_PASSCRED | SO_REUSEADDR => Ok(()),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    fn poll(&self, mask: PollFlags) -> EResult<PollFlags> {
        let inner = self.inner.lock();
        let mut revents = PollFlags::empty();

        match inner.state {
            State::Listening => {
                if !inner.backlog.is_empty() {
                    revents |= PollFlags::In;
                }
            }
            State::Connected => {
                if inner.recv_buf.get_data_len() > 0 || inner.peer_closed {
                    revents |= PollFlags::In;
                }
                if inner.shutdown.contains(ShutdownFlags::Read) {
                    revents |= PollFlags::Hup;
                }
                if inner.peer_closed {
                    revents |= PollFlags::Hup;
                }
                let peer_can_recv = inner
                    .peer
                    .as_ref()
                    .is_some_and(|p| p.inner.lock().recv_buf.get_available_len() > 0);
                if peer_can_recv && !inner.shutdown.contains(ShutdownFlags::Write) {
                    revents |= PollFlags::Out;
                }
                if inner.shutdown.contains(ShutdownFlags::Write) && inner.peer_closed {
                    revents |= PollFlags::Err;
                }
            }
            _ => {
                revents |= PollFlags::Out;
            }
        }

        Ok(revents & (mask | PollFlags::Err | PollFlags::Hup))
    }

    fn poll_events(&self, mask: PollFlags) -> PollEventSet<'_> {
        let wants_read = mask.intersects(PollFlags::Read);
        let wants_write = mask.intersects(PollFlags::Write);

        let mut events = PollEventSet::new();
        if wants_read || !wants_write {
            events = events.add(&self.rd_event);
        }
        if wants_write || !wants_read {
            events = events.add(&self.wr_event);
        }
        events
    }

    fn release(&self) -> EResult<()> {
        let peer = {
            let mut inner = self.inner.lock();
            inner.state = State::Unconnected;
            inner.peer.take()
        };

        if let Some(peer) = peer {
            peer.inner.lock().peer_closed = true;
            peer.rd_event.wake_all();
            peer.wr_event.wake_all();
        }

        Ok(())
    }
}
