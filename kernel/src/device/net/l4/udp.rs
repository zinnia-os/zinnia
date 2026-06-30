//! UDP transport and AF_INET datagram sockets.

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
    util::{event::Event, mutex::spin::SpinMutex},
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
    sync::atomic::{AtomicU16, Ordering},
};

const UDP_HEADER_LEN: usize = 8;
const MAX_UDP_PAYLOAD_LEN: usize = MAX_IPV4_PAYLOAD_LEN - UDP_HEADER_LEN;
const UDP_RECV_QUEUE_LIMIT: usize = 64;
const EPHEMERAL_START: u16 = 49152;
const EPHEMERAL_END: u16 = 65535;

static NEXT_EPHEMERAL: AtomicU16 = AtomicU16::new(0);
static UDP_PORTS: SpinMutex<BTreeMap<u16, Weak<UdpSocket>>> = SpinMutex::new(BTreeMap::new());

struct UdpDatagram {
    source: Ipv4Endpoint,
    data: Vec<u8>,
}

struct UdpInner {
    local: Ipv4Endpoint,
    peer: Option<Ipv4Endpoint>,
    bound: bool,
    bound_device: Option<Arc<ManagedInterface>>,
    recv_queue: VecDeque<UdpDatagram>,
    shutdown: ShutdownFlags,
    self_ref: Weak<UdpSocket>,
}

pub struct UdpSocket {
    inner: SpinMutex<UdpInner>,
    rd_event: Event,
    wr_event: Event,
}

impl UdpSocket {
    pub fn new(sock_type: u32, protocol: i32) -> EResult<Arc<Self>> {
        if sock_type != SOCK_DGRAM {
            return Err(Errno::ESOCKTNOSUPPORT);
        }
        if protocol as u32 != IPPROTO_IP && protocol as u32 != IPPROTO_UDP {
            return Err(Errno::EPROTONOSUPPORT);
        }

        let socket = Arc::try_new(Self {
            inner: SpinMutex::new(UdpInner {
                local: Ipv4Endpoint {
                    addr: Ipv4Addr::ANY,
                    port: 0,
                },
                peer: None,
                bound: false,
                bound_device: None,
                recv_queue: VecDeque::new(),
                shutdown: ShutdownFlags::empty(),
                self_ref: Weak::new(),
            }),
            rd_event: Event::new(),
            wr_event: Event::new(),
        })?;
        socket.inner.lock().self_ref = Arc::downgrade(&socket);
        Ok(socket)
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
            let endpoint = Ipv4Endpoint {
                addr: Ipv4Addr::ANY,
                port,
            };
            if self.bind_endpoint(endpoint).is_ok() {
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
            && endpoint.addr != Ipv4Addr::BROADCAST
            && interface::interface_for_source(endpoint.addr).is_none()
        {
            return Err(Errno::EADDRNOTAVAIL);
        }

        let mut ports = UDP_PORTS.lock();
        if ports
            .get(&endpoint.port)
            .is_some_and(|weak| weak.upgrade().is_some())
        {
            return Err(Errno::EADDRINUSE);
        }
        ports.remove(&endpoint.port);

        let self_ref = {
            let mut inner = self.inner.lock();
            if inner.bound {
                return Err(Errno::EINVAL);
            }
            inner.local = endpoint;
            inner.bound = true;
            inner.self_ref.clone()
        };
        ports.insert(endpoint.port, self_ref);
        Ok(())
    }

    fn local_for_send(&self, interface: &ManagedInterface) -> Ipv4Endpoint {
        let local = self.inner.lock().local;
        Ipv4Endpoint {
            addr: if local.addr == Ipv4Addr::ANY || local.addr == Ipv4Addr::BROADCAST {
                interface.ip()
            } else {
                local.addr
            },
            port: local.port,
        }
    }

    fn send_datagram(&self, destination: Ipv4Endpoint, data: &[u8]) -> EResult<()> {
        if data.len() > MAX_UDP_PAYLOAD_LEN {
            return Err(Errno::EMSGSIZE);
        }
        if destination.addr == Ipv4Addr::ANY || destination.port == 0 {
            return Err(Errno::EDESTADDRREQ);
        }

        self.autobind()?;

        let (bound_addr, bound_device) = {
            let inner = self.inner.lock();
            (inner.local.addr, inner.bound_device.clone())
        };
        let interface = if let Some(device) = bound_device {
            device
        } else if bound_addr == Ipv4Addr::ANY || bound_addr == Ipv4Addr::BROADCAST {
            interface::default_ipv4_interface().ok_or(Errno::ENETUNREACH)?
        } else {
            interface::interface_for_source(bound_addr).ok_or(Errno::ENETUNREACH)?
        };

        let source = self.local_for_send(&interface);
        let mut packet = vec![0u8; UDP_HEADER_LEN + data.len()];
        let packet_len = packet.len();
        packet[0..2].copy_from_slice(&source.port.to_be_bytes());
        packet[2..4].copy_from_slice(&destination.port.to_be_bytes());
        packet[4..6].copy_from_slice(&(packet_len as u16).to_be_bytes());
        packet[6..8].copy_from_slice(&0u16.to_be_bytes());
        packet[UDP_HEADER_LEN..].copy_from_slice(data);

        let sum = udp_checksum(source.addr, destination.addr, &packet);
        packet[6..8].copy_from_slice(&if sum == 0 { 0xffffu16 } else { sum }.to_be_bytes());

        crate::device::net::l3::ipv4::send_packet(
            &interface,
            destination.addr,
            Ipv4Protocol::Udp,
            &packet,
        )
    }
}

impl SocketOps for UdpSocket {
    fn bind(&self, addr: &[u8], _socket: &Arc<Socket>) -> EResult<()> {
        self.bind_endpoint(Self::parse_sockaddr(addr)?)
    }

    fn listen(&self, _backlog: i32) -> EResult<()> {
        Err(Errno::ENOTSUP)
    }

    fn accept(&self, _nonblocking: bool) -> EResult<Arc<Socket>> {
        Err(Errno::ENOTSUP)
    }

    fn connect(&self, addr: &[u8], _nonblocking: bool) -> EResult<()> {
        let endpoint = Self::parse_sockaddr(addr)?;
        if endpoint.addr == Ipv4Addr::ANY || endpoint.port == 0 {
            return Err(Errno::EINVAL);
        }
        self.autobind()?;
        self.inner.lock().peer = Some(endpoint);
        Ok(())
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
        _nonblocking: bool,
    ) -> EResult<isize> {
        let _ = control;
        if self.inner.lock().shutdown.contains(ShutdownFlags::Write) {
            return Err(Errno::EPIPE);
        }

        let destination = match addr {
            Some(addr) => Self::parse_sockaddr(addr)?,
            None => self.inner.lock().peer.ok_or(Errno::EDESTADDRREQ)?,
        };

        let len = buf.len() - buf.total_offset();
        let mut data = vec![0u8; len];
        buf.copy_to_slice(&mut data)?;
        self.send_datagram(destination, &data)?;
        Ok(len as isize)
    }

    fn recvmsg(
        &self,
        buf: &mut IovecIter,
        addr: Option<&mut [u8]>,
        control: &mut [u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<(isize, usize, usize, u32)> {
        let _ = control;
        let peek = flags & MSG_PEEK != 0;

        loop {
            let rd_guard = self.rd_event.guard();
            {
                let mut inner = self.inner.lock();
                if inner.shutdown.contains(ShutdownFlags::Read) {
                    return Ok((0, 0, 0, 0));
                }

                if let Some(datagram) = inner.recv_queue.front() {
                    let copy_len = min(buf.len() - buf.total_offset(), datagram.data.len());
                    if copy_len > 0 {
                        buf.copy_from_slice(&datagram.data[..copy_len])?;
                    }
                    let name_len = match addr {
                        Some(addr) => Self::write_sockaddr(datagram.source, addr),
                        None => 0,
                    };
                    let mut out_flags = 0;
                    if copy_len < datagram.data.len() {
                        out_flags |= MSG_TRUNC;
                    }
                    let len = copy_len as isize;
                    if !peek {
                        inner.recv_queue.pop_front();
                    }
                    return Ok((len, name_len, 0, out_flags));
                }
            }

            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            rd_guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn shutdown(&self, how: u32) -> EResult<()> {
        let flags = ShutdownFlags::from_bits_truncate(how);
        self.inner.lock().shutdown |= flags;
        if flags.contains(ShutdownFlags::Read) {
            self.rd_event.wake_all();
        }
        if flags.contains(ShutdownFlags::Write) {
            self.wr_event.wake_all();
        }
        Ok(())
    }

    fn getsockname(&self, buf: &mut [u8]) -> EResult<usize> {
        let local = self.inner.lock().local;
        Ok(Self::write_sockaddr(local, buf))
    }

    fn getpeername(&self, buf: &mut [u8]) -> EResult<usize> {
        let peer = self.inner.lock().peer.ok_or(Errno::ENOTCONN)?;
        Ok(Self::write_sockaddr(peer, buf))
    }

    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }

        let val = match optname as u32 {
            SO_TYPE => SOCK_DGRAM as i32,
            SO_ERROR => 0,
            SO_SNDBUF | SO_RCVBUF => MAX_UDP_PAYLOAD_LEN as i32,
            SO_DOMAIN => AF_INET as i32,
            SO_PROTOCOL => IPPROTO_UDP as i32,
            _ => return Err(Errno::ENOPROTOOPT),
        };
        let bytes = val.to_ne_bytes();
        let len = min(bytes.len(), buf.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok(size_of::<i32>())
    }

    fn setsockopt(&self, level: i32, optname: i32, buf: &[u8]) -> EResult<()> {
        if level as u32 == SOL_IP {
            return Ok(());
        }
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        match optname as u32 {
            SO_BINDTODEVICE => {
                let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let name = &buf[..end];
                self.inner.lock().bound_device = if name.is_empty() {
                    None
                } else {
                    Some(interface::by_name(name).ok_or(Errno::ENODEV)?)
                };
                Ok(())
            }
            SO_SNDBUF | SO_RCVBUF | SO_REUSEADDR | SO_BROADCAST => Ok(()),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    fn poll(&self, mask: PollFlags) -> EResult<PollFlags> {
        let inner = self.inner.lock();
        let mut revents = PollFlags::empty();
        if !inner.recv_queue.is_empty() || inner.shutdown.contains(ShutdownFlags::Read) {
            revents |= PollFlags::In;
        }
        if !inner.shutdown.contains(ShutdownFlags::Write) {
            revents |= PollFlags::Out;
        }
        Ok(revents & (mask | PollFlags::Err | PollFlags::Hup))
    }

    fn poll_events(&self, mask: PollFlags) -> PollEventSet<'_> {
        let mut events = PollEventSet::new();
        if mask.wants_read_wake() {
            events = events.add(&self.rd_event);
        }
        if mask.wants_write_wake() {
            events = events.add(&self.wr_event);
        }
        events
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let port = {
            let mut inner = self.inner.lock();
            inner.recv_queue.clear();
            if inner.bound {
                inner.bound = false;
                Some(inner.local.port)
            } else {
                None
            }
        };
        if let Some(port) = port {
            UDP_PORTS.lock().remove(&port);
        }
    }
}

pub fn process_packet(interface: &ManagedInterface, ipv4: &Ipv4Header<'_>) -> EResult<bool> {
    let packet = ipv4.payload();
    if packet.len() < UDP_HEADER_LEN {
        return Ok(false);
    }

    let len = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    if len < UDP_HEADER_LEN || len > packet.len() {
        return Ok(false);
    }

    let checksum = u16::from_be_bytes([packet[6], packet[7]]);
    let checksum_result = udp_checksum(ipv4.source(), ipv4.destination(), &packet[..len]);
    if checksum != 0 && checksum_result != 0 {
        return Ok(false);
    }

    let source = Ipv4Endpoint {
        addr: ipv4.source(),
        port: u16::from_be_bytes([packet[0], packet[1]]),
    };
    let destination = Ipv4Endpoint {
        addr: ipv4.destination(),
        port: u16::from_be_bytes([packet[2], packet[3]]),
    };
    let payload = &packet[UDP_HEADER_LEN..len];

    let socket = {
        let mut ports = UDP_PORTS.lock();
        match ports.get(&destination.port).and_then(Weak::upgrade) {
            Some(socket) => socket,
            None => {
                ports.remove(&destination.port);
                return Ok(false);
            }
        }
    };

    let mut inner = socket.inner.lock();
    if inner.local.addr != Ipv4Addr::ANY
        && inner.local.addr != interface.ip()
        && inner.local.addr != Ipv4Addr::BROADCAST
    {
        return Ok(false);
    }
    if let Some(device) = &inner.bound_device
        && !core::ptr::eq(Arc::as_ptr(device), interface as *const ManagedInterface)
    {
        return Ok(false);
    }
    if inner.peer.is_some_and(|peer| peer != source) {
        return Ok(false);
    }
    if inner.recv_queue.len() >= UDP_RECV_QUEUE_LIMIT {
        return Ok(true);
    }
    inner.recv_queue.push_back(UdpDatagram {
        source,
        data: payload.to_vec(),
    });
    drop(inner);
    socket.rd_event.wake_all();
    Ok(true)
}

fn udp_checksum(source: Ipv4Addr, destination: Ipv4Addr, packet: &[u8]) -> u16 {
    let mut sum = 0u32;
    add_bytes(&mut sum, source.as_bytes());
    add_bytes(&mut sum, destination.as_bytes());
    add_bytes(&mut sum, &[0, Ipv4Protocol::Udp.as_u8()]);
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
