//! TCP transport and AF_INET stream sockets.

use crate::{
    device::net::{
        ShutdownFlags, Socket, SocketOps,
        interface::{self, MAX_IPV4_PAYLOAD_LEN, ManagedInterface},
        l3::ipv4::{Ipv4Addr, Ipv4Endpoint, Ipv4Header, Ipv4Protocol},
    },
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::socket::*,
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::file::{PollEventSet, PollFlags},
};
use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::{
    cmp::min,
    sync::atomic::{AtomicU16, AtomicU32, Ordering},
};

const TCP_HEADER_LEN: usize = 20;
const MAX_TCP_PAYLOAD_LEN: usize = MAX_IPV4_PAYLOAD_LEN - TCP_HEADER_LEN;
const TCP_RECV_BUFFER_SIZE: usize = 65536;
const EPHEMERAL_START: u16 = 49152;
const EPHEMERAL_END: u16 = 65535;

const FIN: u16 = 0x01;
const SYN: u16 = 0x02;
const RST: u16 = 0x04;
const ACK: u16 = 0x10;

static NEXT_EPHEMERAL: AtomicU16 = AtomicU16::new(0);
static NEXT_ISN: AtomicU32 = AtomicU32::new(0x1234_0000);
static TCP_PORTS: SpinMutex<BTreeMap<u16, Weak<TcpSocket>>> = SpinMutex::new(BTreeMap::new());
static TCP_CONNECTIONS: SpinMutex<BTreeMap<TcpKey, Weak<TcpSocket>>> =
    SpinMutex::new(BTreeMap::new());
static TCP_CLOSING: SpinMutex<Vec<Arc<TcpSocket>>> = SpinMutex::new(Vec::new());

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TcpKey {
    local: Ipv4Endpoint,
    remote: Ipv4Endpoint,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TcpState {
    Closed,
    Bound,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

struct TcpInner {
    local: Ipv4Endpoint,
    peer: Option<Ipv4Endpoint>,
    bound: bool,
    port_registered: bool,
    state: TcpState,
    pending: VecDeque<Arc<TcpSocket>>,
    backlog: VecDeque<Arc<Socket>>,
    backlog_limit: usize,
    recv_buf: RingBuffer,
    shutdown: ShutdownFlags,
    peer_closed: bool,
    error: Option<Errno>,
    self_ref: Weak<TcpSocket>,
    listener: Option<Weak<TcpSocket>>,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    rcv_nxt: u32,
    peer_mss: usize,
    peer_window: usize,
}

impl TcpInner {
    fn recv_window(&self) -> u16 {
        self.recv_buf.get_available_len().min(u16::MAX as usize) as u16
    }

    fn send_window_available(&self) -> usize {
        let in_flight = self.snd_nxt.wrapping_sub(self.snd_una) as usize;
        self.peer_window.saturating_sub(in_flight)
    }
}

pub struct TcpSocket {
    inner: SpinMutex<TcpInner>,
    rd_event: Event,
    wr_event: Event,
    accept_event: Event,
}

struct TcpSegment<'a> {
    source: Ipv4Endpoint,
    destination: Ipv4Endpoint,
    seq: u32,
    ack: u32,
    flags: u16,
    window: u16,
    options: &'a [u8],
    payload: &'a [u8],
}

impl TcpSegment<'_> {
    fn len(&self) -> u32 {
        let mut len = self.payload.len() as u32;
        if self.flags & SYN != 0 {
            len += 1;
        }
        if self.flags & FIN != 0 {
            len += 1;
        }
        len
    }
}

impl TcpSocket {
    pub fn new(sock_type: u32, protocol: i32) -> EResult<Arc<Self>> {
        if sock_type != SOCK_STREAM {
            return Err(Errno::ESOCKTNOSUPPORT);
        }
        if protocol as u32 != IPPROTO_IP && protocol as u32 != IPPROTO_TCP {
            return Err(Errno::EPROTONOSUPPORT);
        }

        make_socket(TcpState::Closed)
    }

    fn parse_sockaddr(addr: &[u8]) -> EResult<Ipv4Endpoint> {
        if addr.len() < size_of::<sockaddr_in>() {
            return Err(Errno::EINVAL);
        }
        let family = sa_family_t::from_ne_bytes([addr[0], addr[1]]);
        if family as u32 != AF_INET {
            return Err(Errno::EAFNOSUPPORT);
        }

        Ok(Ipv4Endpoint {
            addr: Ipv4Addr::new([addr[4], addr[5], addr[6], addr[7]]),
            port: u16::from_be_bytes([addr[2], addr[3]]),
        })
    }

    fn write_sockaddr(endpoint: Ipv4Endpoint, buf: &mut [u8]) -> usize {
        let mut addr = [0u8; size_of::<sockaddr_in>()];
        addr[0..2].copy_from_slice(&(AF_INET as sa_family_t).to_ne_bytes());
        addr[2..4].copy_from_slice(&endpoint.port.to_be_bytes());
        addr[4..8].copy_from_slice(endpoint.addr.as_bytes());
        let len = min(buf.len(), addr.len());
        buf[..len].copy_from_slice(&addr[..len]);
        addr.len()
    }

    fn autobind(&self) -> EResult<()> {
        if self.inner.lock().bound {
            return Ok(());
        }

        let range = EPHEMERAL_END - EPHEMERAL_START + 1;
        for _ in 0..range {
            let offset = NEXT_EPHEMERAL.fetch_add(1, Ordering::Relaxed) % range;
            let port = EPHEMERAL_START + offset;
            if self
                .bind_endpoint(Ipv4Endpoint {
                    addr: Ipv4Addr::ANY,
                    port,
                })
                .is_ok()
            {
                return Ok(());
            }
        }

        Err(Errno::EADDRINUSE)
    }

    fn bind_endpoint(&self, endpoint: Ipv4Endpoint) -> EResult<()> {
        if endpoint.port == 0 {
            return self.autobind();
        }
        if endpoint.addr != Ipv4Addr::ANY
            && interface::interface_for_source(endpoint.addr).is_none()
        {
            return Err(Errno::EADDRNOTAVAIL);
        }

        let mut ports = TCP_PORTS.lock();
        if ports
            .get(&endpoint.port)
            .is_some_and(|weak| weak.upgrade().is_some())
        {
            return Err(Errno::EADDRINUSE);
        }
        ports.remove(&endpoint.port);

        let self_ref = {
            let mut inner = self.inner.lock();
            if inner.bound || inner.state != TcpState::Closed {
                return Err(Errno::EINVAL);
            }
            inner.local = endpoint;
            inner.bound = true;
            inner.port_registered = true;
            inner.state = TcpState::Bound;
            inner.self_ref.clone()
        };
        ports.insert(endpoint.port, self_ref);
        Ok(())
    }

    fn connect_common(&self, endpoint: Ipv4Endpoint, nonblocking: bool) -> EResult<()> {
        if endpoint.addr == Ipv4Addr::ANY || endpoint.port == 0 {
            return Err(Errno::EINVAL);
        }

        {
            let inner = self.inner.lock();
            match inner.state {
                TcpState::Established => return Err(Errno::EISCONN),
                TcpState::SynSent => return Err(Errno::EALREADY),
                TcpState::Closed | TcpState::Bound => {}
                _ => return Err(Errno::EINVAL),
            }
        }

        self.autobind()?;

        let bound_addr = self.inner.lock().local.addr;
        let interface = if bound_addr == Ipv4Addr::ANY {
            interface::default_ipv4_interface()
        } else {
            interface::interface_for_source(bound_addr)
        }
        .ok_or(Errno::ENETUNREACH)?;

        let local = Ipv4Endpoint {
            addr: if bound_addr == Ipv4Addr::ANY {
                interface.ip()
            } else {
                bound_addr
            },
            port: self.inner.lock().local.port,
        };
        let iss = next_isn();
        let self_ref = {
            let mut inner = self.inner.lock();
            inner.local = local;
            inner.peer = Some(endpoint);
            inner.state = TcpState::SynSent;
            inner.iss = iss;
            inner.snd_una = iss;
            inner.snd_nxt = iss.wrapping_add(1);
            inner.rcv_nxt = 0;
            inner.self_ref.clone()
        };
        register_connection(local, endpoint, self_ref);

        let window = self.inner.lock().recv_window();
        if let Err(e) = send_segment_raw(&interface, local, endpoint, iss, 0, SYN, window, &[]) {
            unregister_connection(local, endpoint);
            self.inner.lock().state = TcpState::Bound;
            return Err(e);
        }

        if nonblocking {
            return Err(Errno::EINPROGRESS);
        }

        let guard = self.wr_event.guard();
        loop {
            {
                let inner = self.inner.lock();
                match inner.state {
                    TcpState::Established => return Ok(()),
                    TcpState::Closed => return Err(inner.error.clone().unwrap()),
                    _ => {}
                }
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn send_current(&self, flags: u16, payload: &[u8]) -> EResult<()> {
        let (local, peer, seq, ack, window) = {
            let inner = self.inner.lock();
            let peer = inner.peer.ok_or(Errno::ENOTCONN)?;
            (
                inner.local,
                peer,
                inner.snd_nxt,
                inner.rcv_nxt,
                inner.recv_window(),
            )
        };
        send_segment(local, peer, seq, ack, flags, window, payload)
    }

    fn send_ack(&self) -> EResult<()> {
        self.send_current(ACK, &[])
    }

    fn send_fin(&self) -> EResult<()> {
        let (local, peer, seq, ack, window) = {
            let mut inner = self.inner.lock();
            if inner.shutdown.contains(ShutdownFlags::Write) {
                return Ok(());
            }
            let peer = inner.peer.ok_or(Errno::ENOTCONN)?;
            inner.shutdown |= ShutdownFlags::Write;
            let seq = inner.snd_nxt;
            inner.snd_nxt = inner.snd_nxt.wrapping_add(1);
            inner.state = match inner.state {
                TcpState::Established => TcpState::FinWait1,
                TcpState::CloseWait => TcpState::LastAck,
                other => other,
            };
            (inner.local, peer, seq, inner.rcv_nxt, inner.recv_window())
        };
        send_segment(local, peer, seq, ack, ACK | FIN, window, &[])
    }

    fn process_segment(self: &Arc<Self>, segment: &TcpSegment<'_>) -> EResult<bool> {
        if segment.flags & RST != 0 {
            self.close_with_error(Errno::ECONNRESET);
            return Ok(true);
        }

        let mut ack_after = false;
        let mut finish_accept = false;
        let mut unregister = None;
        let mut drop_close_ref = false;

        {
            let mut inner = self.inner.lock();
            match inner.state {
                TcpState::SynSent => {
                    if segment.flags & (SYN | ACK) == (SYN | ACK) && segment.ack == inner.snd_nxt {
                        inner.rcv_nxt = segment.seq.wrapping_add(1);
                        inner.snd_una = segment.ack;
                        inner.peer_mss = parse_mss(segment.options);
                        inner.peer_window = segment.window as usize;
                        inner.state = TcpState::Established;
                        ack_after = true;
                        self.wr_event.wake_all();
                    }
                }
                TcpState::SynReceived => {
                    if segment.flags & ACK != 0 && segment.ack == inner.snd_nxt {
                        inner.snd_una = segment.ack;
                        inner.peer_window = segment.window as usize;
                        inner.state = TcpState::Established;
                        finish_accept = true;
                        self.wr_event.wake_all();
                    }
                }
                TcpState::Established
                | TcpState::FinWait1
                | TcpState::FinWait2
                | TcpState::CloseWait
                | TcpState::Closing
                | TcpState::LastAck
                | TcpState::TimeWait => {
                    if segment.flags & ACK != 0 {
                        inner.peer_window = segment.window as usize;
                        if seq_between(segment.ack, inner.snd_una, inner.snd_nxt) {
                            inner.snd_una = segment.ack;
                            match inner.state {
                                TcpState::FinWait1 if segment.ack == inner.snd_nxt => {
                                    inner.state = if inner.peer_closed {
                                        TcpState::TimeWait
                                    } else {
                                        TcpState::FinWait2
                                    };
                                }
                                TcpState::Closing if segment.ack == inner.snd_nxt => {
                                    inner.state = TcpState::TimeWait;
                                }
                                TcpState::LastAck if segment.ack == inner.snd_nxt => {
                                    inner.state = TcpState::Closed;
                                    unregister = inner.peer.map(|peer| (inner.local, peer));
                                    drop_close_ref = true;
                                }
                                _ => {}
                            }
                            self.wr_event.wake_all();
                        }
                    }

                    if segment.seq == inner.rcv_nxt && !segment.payload.is_empty() {
                        let writable =
                            min(segment.payload.len(), inner.recv_buf.get_available_len());
                        if writable > 0 {
                            inner.recv_buf.write(&segment.payload[..writable]);
                            inner.rcv_nxt = inner.rcv_nxt.wrapping_add(writable as u32);
                            ack_after = true;
                            self.rd_event.wake_one();
                        }
                    }

                    if segment.seq.wrapping_add(segment.payload.len() as u32) == inner.rcv_nxt
                        && segment.flags & FIN != 0
                    {
                        inner.rcv_nxt = inner.rcv_nxt.wrapping_add(1);
                        inner.peer_closed = true;
                        inner.shutdown |= ShutdownFlags::Read;
                        inner.state = match inner.state {
                            TcpState::Established => TcpState::CloseWait,
                            TcpState::FinWait1 => TcpState::Closing,
                            TcpState::FinWait2 => TcpState::TimeWait,
                            other => other,
                        };
                        if inner.state == TcpState::TimeWait {
                            unregister = inner.peer.map(|peer| (inner.local, peer));
                            drop_close_ref = true;
                        }
                        ack_after = true;
                        self.rd_event.wake_all();
                    }
                }
                _ => {}
            }
        }

        if finish_accept {
            self.finish_accept()?;
        }
        if ack_after {
            self.send_ack()?;
        }
        if let Some((local, peer)) = unregister {
            unregister_connection(local, peer);
        }
        if drop_close_ref {
            remove_closing_ref(self);
        }

        Ok(true)
    }

    fn finish_accept(self: &Arc<Self>) -> EResult<()> {
        let listener = {
            let inner = self.inner.lock();
            inner.listener.as_ref().and_then(Weak::upgrade)
        };
        let Some(listener) = listener else {
            return Ok(());
        };

        let socket = Socket::new(AF_INET, SOCK_STREAM, self.clone())?;
        {
            let mut listener_inner = listener.inner.lock();
            if listener_inner.state != TcpState::Listen {
                return Ok(());
            }
            listener_inner
                .pending
                .retain(|pending| !Arc::ptr_eq(pending, self));
            if listener_inner.backlog.len() >= listener_inner.backlog_limit {
                self.close_with_error(Errno::ECONNABORTED);
                return Ok(());
            }
            listener_inner.backlog.push_back(socket);
        }
        listener.accept_event.wake_one();
        listener.rd_event.wake_one();
        Ok(())
    }

    fn close_with_error(&self, errno: Errno) {
        let key = {
            let mut inner = self.inner.lock();
            inner.state = TcpState::Closed;
            inner.error = Some(errno);
            inner.peer_closed = true;
            inner.recv_buf.clear();
            inner.peer.map(|peer| (inner.local, peer))
        };
        if let Some((local, peer)) = key {
            unregister_connection(local, peer);
        }
        self.rd_event.wake_all();
        self.wr_event.wake_all();
        self.accept_event.wake_all();
    }
}

impl SocketOps for TcpSocket {
    fn bind(&self, addr: &[u8], _socket: &Arc<Socket>) -> EResult<()> {
        self.bind_endpoint(Self::parse_sockaddr(addr)?)
    }

    fn listen(&self, backlog: i32) -> EResult<()> {
        self.autobind()?;
        let mut inner = self.inner.lock();
        match inner.state {
            TcpState::Bound | TcpState::Listen => {
                inner.state = TcpState::Listen;
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
                if inner.state != TcpState::Listen {
                    return Err(Errno::EINVAL);
                }
                if let Some(socket) = inner.backlog.pop_front() {
                    return Ok(socket);
                }
            }
            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn connect(&self, addr: &[u8], nonblocking: bool) -> EResult<()> {
        self.connect_common(Self::parse_sockaddr(addr)?, nonblocking)
    }

    fn send(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize> {
        self.sendmsg(buf, None, &[], flags, nonblocking)
    }

    fn recv(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize> {
        let (n, _, _, _) = self.recvmsg(buf, None, &mut [], flags, nonblocking)?;
        Ok(n)
    }

    fn sendmsg(
        &self,
        buf: &mut IovecIter,
        addr: Option<&[u8]>,
        control: &[u8],
        _flags: u32,
        nonblocking: bool,
    ) -> EResult<isize> {
        let _ = (addr, control);
        if buf.is_empty() {
            return Ok(0);
        }

        let mut sent = 0usize;
        while !buf.is_finished() {
            let chunk_len = loop {
                let guard = self.wr_event.guard();
                {
                    let inner = self.inner.lock();
                    if !matches!(inner.state, TcpState::Established | TcpState::CloseWait) {
                        return if sent > 0 {
                            Ok(sent as isize)
                        } else {
                            Err(Errno::ENOTCONN)
                        };
                    }
                    if inner.shutdown.contains(ShutdownFlags::Write) {
                        return if sent > 0 {
                            Ok(sent as isize)
                        } else {
                            Err(Errno::EPIPE)
                        };
                    }

                    let available = inner.send_window_available();
                    if available > 0 {
                        break min(
                            buf.len() - buf.total_offset(),
                            min(inner.peer_mss, available),
                        );
                    }
                }

                if nonblocking {
                    return if sent > 0 {
                        Ok(sent as isize)
                    } else {
                        Err(Errno::EAGAIN)
                    };
                }
                guard.wait();
                if Scheduler::get_current().has_pending_signals() {
                    return if sent > 0 {
                        Ok(sent as isize)
                    } else {
                        Err(Errno::EINTR)
                    };
                };
            };
            let mut data = vec![0u8; chunk_len];
            buf.copy_to_slice(&mut data)?;

            let (local, peer, seq, ack, window) = {
                let mut inner = self.inner.lock();
                let peer = inner.peer.ok_or(Errno::ENOTCONN)?;
                let seq = inner.snd_nxt;
                inner.snd_nxt = inner.snd_nxt.wrapping_add(data.len() as u32);
                (inner.local, peer, seq, inner.rcv_nxt, inner.recv_window())
            };
            send_segment(local, peer, seq, ack, ACK, window, &data)?;
            sent += data.len();
        }

        Ok(sent as isize)
    }

    fn recvmsg(
        &self,
        buf: &mut IovecIter,
        addr: Option<&mut [u8]>,
        control: &mut [u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<(isize, usize, usize, u32)> {
        let _ = (addr, control);
        let peek = flags & MSG_PEEK != 0;
        let guard = self.rd_event.guard();

        loop {
            {
                let mut inner = self.inner.lock();
                if inner.state == TcpState::Listen || inner.state == TcpState::SynSent {
                    return Err(Errno::ENOTCONN);
                }

                let available = inner.recv_buf.get_data_len();
                if available > 0 {
                    let len = min(buf.len() - buf.total_offset(), available);
                    let mut data = vec![0u8; len];
                    let got = if peek {
                        inner.recv_buf.peek(&mut data)
                    } else {
                        inner.recv_buf.read(&mut data)
                    };
                    if got > 0 {
                        buf.copy_from_slice(&data[..got])?;
                        if !peek {
                            self.wr_event.wake_one();
                        }
                    }
                    return Ok((got as isize, 0, 0, 0));
                }

                if inner.peer_closed
                    || inner.shutdown.contains(ShutdownFlags::Read)
                    || matches!(inner.state, TcpState::Closed | TcpState::TimeWait)
                {
                    return Ok((0, 0, 0, 0));
                }
            }

            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn shutdown(&self, how: u32) -> EResult<()> {
        let flags = ShutdownFlags::from_bits_truncate(how);
        if flags.contains(ShutdownFlags::Read) {
            let mut inner = self.inner.lock();
            inner.shutdown |= ShutdownFlags::Read;
            inner.peer_closed = true;
            drop(inner);
            self.rd_event.wake_all();
        }
        if flags.contains(ShutdownFlags::Write) {
            self.send_fin()?;
            self.wr_event.wake_all();
        }
        Ok(())
    }

    fn getsockname(&self, buf: &mut [u8]) -> EResult<usize> {
        let local = self.inner.lock().local;
        Ok(Self::write_sockaddr(local, buf))
    }

    fn getpeername(&self, buf: &mut [u8]) -> EResult<usize> {
        let inner = self.inner.lock();
        let peer = inner.peer.ok_or(Errno::ENOTCONN)?;
        if matches!(
            inner.state,
            TcpState::Closed | TcpState::Bound | TcpState::Listen
        ) {
            return Err(Errno::ENOTCONN);
        }
        Ok(Self::write_sockaddr(peer, buf))
    }

    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }

        let val = match optname as u32 {
            SO_TYPE => SOCK_STREAM as i32,
            SO_ERROR => self
                .inner
                .lock()
                .error
                .as_ref()
                .map(|x| x.clone() as i32)
                .unwrap_or(0),
            SO_SNDBUF | SO_RCVBUF => TCP_RECV_BUFFER_SIZE as i32,
            SO_DOMAIN => AF_INET as i32,
            SO_PROTOCOL => IPPROTO_TCP as i32,
            SO_ACCEPTCONN => (self.inner.lock().state == TcpState::Listen) as i32,
            _ => return Err(Errno::ENOPROTOOPT),
        };
        let bytes = val.to_ne_bytes();
        let len = min(bytes.len(), buf.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok(size_of::<i32>())
    }

    fn setsockopt(&self, level: i32, optname: i32, _buf: &[u8]) -> EResult<()> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        match optname as u32 {
            SO_SNDBUF | SO_RCVBUF | SO_REUSEADDR | SO_KEEPALIVE => Ok(()),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    fn poll(&self, mask: PollFlags) -> EResult<PollFlags> {
        let inner = self.inner.lock();
        let mut revents = PollFlags::empty();

        match inner.state {
            TcpState::Listen => {
                if !inner.backlog.is_empty() {
                    revents |= PollFlags::In;
                }
            }
            TcpState::SynSent => {}
            TcpState::Established
            | TcpState::CloseWait
            | TcpState::FinWait1
            | TcpState::FinWait2 => {
                if inner.recv_buf.get_data_len() > 0 || inner.peer_closed {
                    revents |= PollFlags::In;
                }
                if !inner.shutdown.contains(ShutdownFlags::Write)
                    && matches!(inner.state, TcpState::Established | TcpState::CloseWait)
                {
                    revents |= PollFlags::Out;
                }
                if inner.peer_closed {
                    revents |= PollFlags::Hup;
                }
            }
            TcpState::Closed => {
                if inner.error.is_some() {
                    revents |= PollFlags::Err;
                }
            }
            _ => {}
        }

        Ok(revents & (mask | PollFlags::Err | PollFlags::Hup))
    }

    fn poll_events(&self, mask: PollFlags) -> PollEventSet<'_> {
        let wants_read = mask.intersects(PollFlags::Read);
        let wants_write = mask.intersects(PollFlags::Write);

        let mut events = PollEventSet::new();
        if wants_read || !wants_write {
            events = events.add(&self.rd_event).add(&self.accept_event);
        }
        if wants_write || !wants_read {
            events = events.add(&self.wr_event);
        }
        events
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        let (local, peer, remove_port, send_fin, close_ref) = {
            let mut inner = self.inner.lock();
            let remove_port = inner.port_registered.then_some(inner.local.port);
            let send_fin = matches!(inner.state, TcpState::Established | TcpState::CloseWait)
                && !inner.shutdown.contains(ShutdownFlags::Write);
            let local = inner.local;
            let peer = inner.peer;
            let close_ref = if send_fin {
                inner.self_ref.upgrade()
            } else {
                None
            };
            if !send_fin {
                inner.state = TcpState::Closed;
            }
            inner.pending.clear();
            inner.backlog.clear();
            inner.recv_buf.clear();
            inner.bound = false;
            inner.port_registered = false;
            (local, peer, remove_port, send_fin, close_ref)
        };

        if send_fin {
            if let Some(socket) = close_ref {
                retain_closing_ref(socket);
            }
            let _ = self.send_fin();
        } else if let Some(peer) = peer {
            unregister_connection(local, peer);
        }
        if let Some(port) = remove_port {
            TCP_PORTS.lock().remove(&port);
        }
        self.rd_event.wake_all();
        self.wr_event.wake_all();
        self.accept_event.wake_all();
    }
}

pub fn process_packet(interface: &ManagedInterface, ipv4: &Ipv4Header<'_>) -> EResult<bool> {
    let Some(segment) = parse_segment(ipv4) else {
        return Ok(false);
    };
    if tcp_checksum(ipv4.source(), ipv4.destination(), ipv4.payload()) != 0 {
        return Ok(false);
    }

    let key = TcpKey {
        local: segment.destination,
        remote: segment.source,
    };
    let socket = {
        let mut connections = TCP_CONNECTIONS.lock();
        match connections.get(&key).and_then(Weak::upgrade) {
            Some(socket) => Some(socket),
            None => {
                connections.remove(&key);
                None
            }
        }
    };
    if let Some(socket) = socket {
        return socket.process_segment(&segment);
    }

    if segment.flags & SYN == 0 || segment.flags & ACK != 0 {
        send_reset(interface, &segment)?;
        return Ok(false);
    }

    let listener = {
        let mut ports = TCP_PORTS.lock();
        match ports.get(&segment.destination.port).and_then(Weak::upgrade) {
            Some(socket) => Some(socket),
            None => {
                ports.remove(&segment.destination.port);
                None
            }
        }
    };
    let Some(listener) = listener else {
        send_reset(interface, &segment)?;
        return Ok(false);
    };

    let listener_ref = {
        let inner = listener.inner.lock();
        if inner.state != TcpState::Listen
            || (inner.local.addr != Ipv4Addr::ANY && inner.local.addr != interface.ip())
            || inner.backlog.len() + inner.pending.len() >= inner.backlog_limit
        {
            return Ok(false);
        }
        inner.self_ref.clone()
    };

    let child = make_socket(TcpState::SynReceived)?;
    let iss = next_isn();
    {
        let mut inner = child.inner.lock();
        inner.local = Ipv4Endpoint {
            addr: interface.ip(),
            port: segment.destination.port,
        };
        inner.peer = Some(segment.source);
        inner.bound = true;
        inner.listener = Some(listener_ref);
        inner.iss = iss;
        inner.snd_una = iss;
        inner.snd_nxt = iss.wrapping_add(1);
        inner.rcv_nxt = segment.seq.wrapping_add(1);
        inner.peer_mss = parse_mss(segment.options);
        inner.peer_window = segment.window as usize;
    }
    let (child_local, child_ref) = {
        let inner = child.inner.lock();
        (inner.local, inner.self_ref.clone())
    };
    listener.inner.lock().pending.push_back(child.clone());
    register_connection(child_local, segment.source, child_ref);
    let send_result = send_segment_raw(
        interface,
        child_local,
        segment.source,
        iss,
        segment.seq.wrapping_add(1),
        SYN | ACK,
        TCP_RECV_BUFFER_SIZE
            .saturating_sub(1)
            .min(u16::MAX as usize) as u16,
        &[],
    );
    if let Err(e) = send_result {
        unregister_connection(child_local, segment.source);
        listener
            .inner
            .lock()
            .pending
            .retain(|pending| !Arc::ptr_eq(pending, &child));
        return Err(e);
    }
    Ok(true)
}

fn make_socket(state: TcpState) -> EResult<Arc<TcpSocket>> {
    let socket = Arc::try_new(TcpSocket {
        inner: SpinMutex::new(TcpInner {
            local: Ipv4Endpoint {
                addr: Ipv4Addr::ANY,
                port: 0,
            },
            peer: None,
            bound: false,
            port_registered: false,
            state,
            pending: VecDeque::new(),
            backlog: VecDeque::new(),
            backlog_limit: 1,
            recv_buf: RingBuffer::new(TCP_RECV_BUFFER_SIZE),
            shutdown: ShutdownFlags::empty(),
            peer_closed: false,
            error: None,
            self_ref: Weak::new(),
            listener: None,
            iss: 0,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            peer_mss: MAX_TCP_PAYLOAD_LEN,
            peer_window: 0,
        }),
        rd_event: Event::new(),
        wr_event: Event::new(),
        accept_event: Event::new(),
    })?;
    socket.inner.lock().self_ref = Arc::downgrade(&socket);
    Ok(socket)
}

fn parse_segment<'a>(ipv4: &'a Ipv4Header<'_>) -> Option<TcpSegment<'a>> {
    let packet = ipv4.payload();
    if packet.len() < TCP_HEADER_LEN {
        return None;
    }
    let header_len = ((packet[12] >> 4) as usize) * 4;
    if header_len < TCP_HEADER_LEN || header_len > packet.len() {
        return None;
    }

    Some(TcpSegment {
        source: Ipv4Endpoint {
            addr: ipv4.source(),
            port: u16::from_be_bytes([packet[0], packet[1]]),
        },
        destination: Ipv4Endpoint {
            addr: ipv4.destination(),
            port: u16::from_be_bytes([packet[2], packet[3]]),
        },
        seq: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
        ack: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
        flags: (((packet[12] as u16) & 0x01) << 8) | packet[13] as u16,
        window: u16::from_be_bytes([packet[14], packet[15]]),
        options: &packet[TCP_HEADER_LEN..header_len],
        payload: &packet[header_len..],
    })
}

fn parse_mss(options: &[u8]) -> usize {
    let mut i = 0;
    while i < options.len() {
        match options[i] {
            0 => break,
            1 => i += 1,
            2 if i + 4 <= options.len() && options[i + 1] == 4 => {
                let mss = u16::from_be_bytes([options[i + 2], options[i + 3]]) as usize;
                return mss.clamp(1, MAX_TCP_PAYLOAD_LEN);
            }
            _ => {
                if i + 1 >= options.len() || options[i + 1] < 2 {
                    break;
                }
                i += options[i + 1] as usize;
            }
        }
    }
    MAX_TCP_PAYLOAD_LEN
}

fn send_segment(
    local: Ipv4Endpoint,
    peer: Ipv4Endpoint,
    seq: u32,
    ack: u32,
    flags: u16,
    window: u16,
    payload: &[u8],
) -> EResult<()> {
    let interface = interface::interface_for_source(local.addr).ok_or(Errno::ENETUNREACH)?;
    send_segment_raw(&interface, local, peer, seq, ack, flags, window, payload)
}

fn send_segment_raw(
    interface: &ManagedInterface,
    local: Ipv4Endpoint,
    peer: Ipv4Endpoint,
    seq: u32,
    ack: u32,
    flags: u16,
    window: u16,
    payload: &[u8],
) -> EResult<()> {
    if payload.len() > MAX_TCP_PAYLOAD_LEN {
        return Err(Errno::EMSGSIZE);
    }

    let mut packet = vec![0u8; TCP_HEADER_LEN + payload.len()];
    packet[0..2].copy_from_slice(&local.port.to_be_bytes());
    packet[2..4].copy_from_slice(&peer.port.to_be_bytes());
    packet[4..8].copy_from_slice(&seq.to_be_bytes());
    packet[8..12].copy_from_slice(&ack.to_be_bytes());
    packet[12] = (5 << 4) | (((flags >> 8) as u8) & 0x01);
    packet[13] = flags as u8;
    packet[14..16].copy_from_slice(&window.to_be_bytes());
    packet[16..18].copy_from_slice(&0u16.to_be_bytes());
    packet[18..20].copy_from_slice(&0u16.to_be_bytes());
    packet[TCP_HEADER_LEN..].copy_from_slice(payload);

    let sum = tcp_checksum(local.addr, peer.addr, &packet);
    packet[16..18].copy_from_slice(&sum.to_be_bytes());

    crate::device::net::l3::ipv4::send_packet(interface, peer.addr, Ipv4Protocol::Tcp, &packet)
}

fn send_reset(interface: &ManagedInterface, segment: &TcpSegment<'_>) -> EResult<()> {
    let (seq, ack, flags) = if segment.flags & ACK != 0 {
        (segment.ack, 0, RST)
    } else {
        let len = segment.len();
        (0, segment.seq.wrapping_add(len), RST | ACK)
    };
    send_segment_raw(
        interface,
        segment.destination,
        segment.source,
        seq,
        ack,
        flags,
        0,
        &[],
    )
}

fn register_connection(local: Ipv4Endpoint, remote: Ipv4Endpoint, socket: Weak<TcpSocket>) {
    TCP_CONNECTIONS
        .lock()
        .insert(TcpKey { local, remote }, socket);
}

fn unregister_connection(local: Ipv4Endpoint, remote: Ipv4Endpoint) {
    TCP_CONNECTIONS.lock().remove(&TcpKey { local, remote });
}

fn retain_closing_ref(socket: Arc<TcpSocket>) {
    let mut closing = TCP_CLOSING.lock();
    if !closing.iter().any(|held| Arc::ptr_eq(held, &socket)) {
        closing.push(socket);
    }
}

fn remove_closing_ref(socket: &Arc<TcpSocket>) {
    TCP_CLOSING.lock().retain(|held| !Arc::ptr_eq(held, socket));
}

fn tcp_checksum(source: Ipv4Addr, destination: Ipv4Addr, packet: &[u8]) -> u16 {
    let mut sum = 0u32;
    add_bytes(&mut sum, source.as_bytes());
    add_bytes(&mut sum, destination.as_bytes());
    add_bytes(&mut sum, &[0, Ipv4Protocol::Tcp.as_u8()]);
    add_bytes(&mut sum, &(packet.len() as u16).to_be_bytes());
    add_bytes(&mut sum, packet);

    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn add_bytes(sum: &mut u32, buf: &[u8]) {
    let mut chunks = buf.chunks_exact(2);
    for chunk in &mut chunks {
        *sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        *sum += (*last as u32) << 8;
    }
}

fn next_isn() -> u32 {
    NEXT_ISN.fetch_add(64_000, Ordering::Relaxed)
}

fn seq_between(seq: u32, low: u32, high: u32) -> bool {
    seq.wrapping_sub(low) <= high.wrapping_sub(low)
}
