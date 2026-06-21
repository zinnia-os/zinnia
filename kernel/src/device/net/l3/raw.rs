use crate::{
    device::net::{
        ShutdownFlags, Socket, SocketOps,
        interface::{self, MAX_IPV4_PAYLOAD_LEN},
        l3::ipv4::{self, IPV4_HEADER_LEN, Ipv4Addr, Ipv4Protocol},
    },
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::socket::*,
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::file::{PollEventSet, PollFlags},
};
use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::cmp::min;

const RAW_RECV_QUEUE_LIMIT: usize = 64;

static RAW_SOCKS: SpinMutex<Vec<Weak<RawSocket>>> = SpinMutex::new(Vec::new());

struct RawDatagram {
    source: Ipv4Addr,
    data: Vec<u8>,
}

struct RawInner {
    protocol: u8,
    peer: Option<Ipv4Addr>,
    recv_queue: VecDeque<RawDatagram>,
    shutdown: ShutdownFlags,
}

pub struct RawSocket {
    inner: SpinMutex<RawInner>,
    rd_event: Event,
    wr_event: Event,
}

impl RawSocket {
    pub fn new(protocol: i32) -> EResult<Arc<Self>> {
        let socket = Arc::try_new(Self {
            inner: SpinMutex::new(RawInner {
                protocol: protocol as u8,
                peer: None,
                recv_queue: VecDeque::new(),
                shutdown: ShutdownFlags::empty(),
            }),
            rd_event: Event::new(),
            wr_event: Event::new(),
        })?;

        let mut socks = RAW_SOCKS.lock();
        socks.retain(|weak| weak.strong_count() > 0);
        socks.push(Arc::downgrade(&socket));
        Ok(socket)
    }

    fn parse_addr(addr: &[u8]) -> EResult<Ipv4Addr> {
        if addr.len() < size_of::<sockaddr_in>() {
            return Err(Errno::EINVAL);
        }
        let family = sa_family_t::from_ne_bytes([addr[0], addr[1]]);
        if family as u32 != AF_INET {
            return Err(Errno::EAFNOSUPPORT);
        }
        Ok(Ipv4Addr::new([addr[4], addr[5], addr[6], addr[7]]))
    }

    fn write_addr(addr: Ipv4Addr, buf: &mut [u8]) -> usize {
        let mut sa = [0u8; size_of::<sockaddr_in>()];
        sa[0..2].copy_from_slice(&(AF_INET as sa_family_t).to_ne_bytes());
        sa[4..8].copy_from_slice(addr.as_bytes());
        let len = min(buf.len(), sa.len());
        buf[..len].copy_from_slice(&sa[..len]);
        sa.len()
    }

    fn send_to(&self, dest: Ipv4Addr, data: &[u8]) -> EResult<isize> {
        if data.len() > MAX_IPV4_PAYLOAD_LEN {
            return Err(Errno::EMSGSIZE);
        }
        if dest == Ipv4Addr::ANY {
            return Err(Errno::EDESTADDRREQ);
        }
        let protocol = Ipv4Protocol::from_u8(self.inner.lock().protocol);
        let interface = interface::default_ipv4_interface().ok_or(Errno::ENETUNREACH)?;
        ipv4::send_packet(&interface, dest, protocol, data)?;
        Ok(data.len() as isize)
    }
}

impl SocketOps for RawSocket {
    fn bind(&self, addr: &[u8], _socket: &Arc<Socket>) -> EResult<()> {
        Self::parse_addr(addr)?;
        Ok(())
    }

    fn connect(&self, addr: &[u8], _nonblocking: bool) -> EResult<()> {
        self.inner.lock().peer = Some(Self::parse_addr(addr)?);
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
        _control: &[u8],
        _flags: u32,
        _nonblocking: bool,
    ) -> EResult<isize> {
        if self.inner.lock().shutdown.contains(ShutdownFlags::Write) {
            return Err(Errno::EPIPE);
        }
        let dest = match addr {
            Some(addr) => Self::parse_addr(addr)?,
            None => self.inner.lock().peer.ok_or(Errno::EDESTADDRREQ)?,
        };
        let len = buf.len() - buf.total_offset();
        let mut data = vec![0u8; len];
        buf.copy_to_slice(&mut data)?;
        self.send_to(dest, &data)
    }

    fn recvmsg(
        &self,
        buf: &mut IovecIter,
        addr: Option<&mut [u8]>,
        _control: &mut [u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<(isize, usize, usize, u32)> {
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
                        Some(addr) => Self::write_addr(datagram.source, addr),
                        None => 0,
                    };
                    let mut out_flags = 0;
                    if copy_len < datagram.data.len() {
                        out_flags |= MSG_TRUNC;
                    }
                    if !peek {
                        inner.recv_queue.pop_front();
                    }
                    return Ok((copy_len as isize, name_len, 0, out_flags));
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
        Ok(Self::write_addr(Ipv4Addr::ANY, buf))
    }

    fn getpeername(&self, buf: &mut [u8]) -> EResult<usize> {
        let peer = self.inner.lock().peer.ok_or(Errno::ENOTCONN)?;
        Ok(Self::write_addr(peer, buf))
    }

    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        let val = match optname as u32 {
            SO_TYPE => SOCK_RAW as i32,
            SO_ERROR => 0,
            SO_SNDBUF | SO_RCVBUF => MAX_IPV4_PAYLOAD_LEN as i32,
            SO_DOMAIN => AF_INET as i32,
            SO_PROTOCOL => self.inner.lock().protocol as i32,
            _ => return Err(Errno::ENOPROTOOPT),
        };
        let bytes = val.to_ne_bytes();
        let len = min(bytes.len(), buf.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok(size_of::<i32>())
    }

    fn setsockopt(&self, level: i32, _optname: i32, _buf: &[u8]) -> EResult<()> {
        match level as u32 {
            SOL_SOCKET | SOL_IP => Ok(()),
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
}

impl Drop for RawSocket {
    fn drop(&mut self) {
        RAW_SOCKS.lock().retain(|weak| weak.strong_count() > 0);
    }
}

pub fn deliver(protocol: Ipv4Protocol, ip_packet: &[u8]) {
    if ip_packet.len() < IPV4_HEADER_LEN {
        return;
    }
    let source = Ipv4Addr::new([ip_packet[12], ip_packet[13], ip_packet[14], ip_packet[15]]);
    let proto = protocol.as_u8();

    let socks: Vec<Arc<RawSocket>> = {
        let guard = RAW_SOCKS.lock();
        guard.iter().filter_map(Weak::upgrade).collect()
    };

    for sock in socks {
        let woke = {
            let mut inner = sock.inner.lock();
            let accept = (inner.protocol == proto || inner.protocol == 0)
                && inner.peer.is_none_or(|peer| peer == source)
                && inner.recv_queue.len() < RAW_RECV_QUEUE_LIMIT;
            if accept {
                inner.recv_queue.push_back(RawDatagram {
                    source,
                    data: ip_packet.to_vec(),
                });
            }
            accept
        };
        if woke {
            sock.rd_event.wake_all();
        }
    }
}
