use crate::{
    device::net::{
        ShutdownFlags, Socket, SocketOps,
        interface::{self, RX_FRAME_LEN},
        l2::eth::ETH_HEADER_LEN,
    },
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::{net::*, socket::*},
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

const PACKET_RECV_QUEUE_LIMIT: usize = 64;
const SOCKADDR_LL_LEN: usize = size_of::<sockaddr_ll>();

static PACKET_SOCKS: SpinMutex<Vec<Weak<PacketSocket>>> = SpinMutex::new(Vec::new());

struct PacketFrame {
    addr: [u8; SOCKADDR_LL_LEN],
    data: Vec<u8>,
}

struct PacketInner {
    ifindex: Option<u32>,
    protocol: u16,
    recv_queue: VecDeque<PacketFrame>,
    shutdown: ShutdownFlags,
}

pub struct PacketSocket {
    inner: SpinMutex<PacketInner>,
    rd_event: Event,
    wr_event: Event,
}

impl PacketSocket {
    pub fn new(sock_type: u32, protocol: i32) -> EResult<Arc<Self>> {
        if sock_type != SOCK_RAW && sock_type != SOCK_DGRAM {
            return Err(Errno::ESOCKTNOSUPPORT);
        }

        let socket = Arc::try_new(Self {
            inner: SpinMutex::new(PacketInner {
                ifindex: None,
                protocol: u16::from_be(protocol as u16),
                recv_queue: VecDeque::new(),
                shutdown: ShutdownFlags::empty(),
            }),
            rd_event: Event::new(),
            wr_event: Event::new(),
        })?;

        let mut socks = PACKET_SOCKS.lock();
        socks.retain(|weak| weak.strong_count() > 0);
        socks.push(Arc::downgrade(&socket));
        Ok(socket)
    }

    fn parse_sockaddr_ll(addr: &[u8]) -> EResult<(Option<u32>, u16)> {
        if addr.len() < SOCKADDR_LL_LEN {
            return Err(Errno::EINVAL);
        }
        let family = u16::from_ne_bytes([addr[0], addr[1]]);
        if family as u32 != AF_PACKET {
            return Err(Errno::EAFNOSUPPORT);
        }
        let protocol = u16::from_be_bytes([addr[2], addr[3]]);
        let ifindex = i32::from_ne_bytes([addr[4], addr[5], addr[6], addr[7]]);
        Ok(((ifindex != 0).then_some(ifindex as u32), protocol))
    }
}

impl SocketOps for PacketSocket {
    fn bind(&self, addr: &[u8], _socket: &Arc<Socket>) -> EResult<()> {
        let (ifindex, protocol) = Self::parse_sockaddr_ll(addr)?;
        let mut inner = self.inner.lock();
        if ifindex.is_some() {
            inner.ifindex = ifindex;
        }
        if protocol != 0 {
            inner.protocol = protocol;
        }
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
        let target = match addr {
            Some(addr) => Self::parse_sockaddr_ll(addr)?.0,
            None => None,
        };
        let ifindex = target.or(self.inner.lock().ifindex).ok_or(Errno::EINVAL)?;
        let interface = interface::by_index(ifindex).ok_or(Errno::ENODEV)?;

        let len = buf.len() - buf.total_offset();
        if len == 0 || len > RX_FRAME_LEN {
            return Err(Errno::EMSGSIZE);
        }
        let mut frame = vec![0u8; len];
        buf.copy_to_slice(&mut frame)?;
        interface.send_raw(&frame)?;
        Ok(len as isize)
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
                if let Some(frame) = inner.recv_queue.front() {
                    let copy_len = min(buf.len() - buf.total_offset(), frame.data.len());
                    if copy_len > 0 {
                        buf.copy_from_slice(&frame.data[..copy_len])?;
                    }
                    let name_len = match addr {
                        Some(addr) => {
                            let n = min(addr.len(), SOCKADDR_LL_LEN);
                            addr[..n].copy_from_slice(&frame.addr[..n]);
                            SOCKADDR_LL_LEN
                        }
                        None => 0,
                    };
                    let mut out_flags = 0;
                    if copy_len < frame.data.len() {
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
        let inner = self.inner.lock();
        let mut sa = [0u8; SOCKADDR_LL_LEN];
        sa[0..2].copy_from_slice(&(AF_PACKET as u16).to_ne_bytes());
        sa[2..4].copy_from_slice(&inner.protocol.to_be_bytes());
        sa[4..8].copy_from_slice(&(inner.ifindex.unwrap_or(0) as i32).to_ne_bytes());
        sa[8..10].copy_from_slice(&ARPHRD_ETHER.to_ne_bytes());
        let n = min(buf.len(), SOCKADDR_LL_LEN);
        buf[..n].copy_from_slice(&sa[..n]);
        Ok(SOCKADDR_LL_LEN)
    }

    fn getpeername(&self, _buf: &mut [u8]) -> EResult<usize> {
        Err(Errno::ENOTCONN)
    }

    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        let val = match optname as u32 {
            SO_TYPE => SOCK_RAW as i32,
            SO_ERROR => 0,
            SO_DOMAIN => AF_PACKET as i32,
            SO_PROTOCOL => self.inner.lock().protocol as i32,
            _ => return Err(Errno::ENOPROTOOPT),
        };
        let bytes = val.to_ne_bytes();
        let len = min(bytes.len(), buf.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok(size_of::<i32>())
    }

    fn setsockopt(&self, level: i32, optname: i32, _buf: &[u8]) -> EResult<()> {
        match (level as u32, optname as u32) {
            (SOL_SOCKET, SO_ATTACH_FILTER | SO_DETACH_FILTER | SO_LOCK_FILTER) => Ok(()),
            (SOL_PACKET, PACKET_AUXDATA) => Ok(()),
            (SOL_SOCKET, SO_RCVBUF | SO_SNDBUF | SO_BROADCAST) => Ok(()),
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

impl Drop for PacketSocket {
    fn drop(&mut self) {
        PACKET_SOCKS.lock().retain(|weak| weak.strong_count() > 0);
    }
}

pub fn deliver(ifindex: u32, frame: &[u8]) {
    if frame.len() < ETH_HEADER_LEN {
        return;
    }
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);

    let dst = &frame[0..6];
    let pkttype = if dst == [0xff; 6] {
        PACKET_BROADCAST
    } else if dst[0] & 0x01 != 0 {
        PACKET_MULTICAST
    } else {
        PACKET_HOST
    };

    let mut addr = [0u8; SOCKADDR_LL_LEN];
    addr[0..2].copy_from_slice(&(AF_PACKET as u16).to_ne_bytes());
    addr[2..4].copy_from_slice(&frame[12..14]);
    addr[4..8].copy_from_slice(&(ifindex as i32).to_ne_bytes());
    addr[8..10].copy_from_slice(&ARPHRD_ETHER.to_ne_bytes());
    addr[10] = pkttype;
    addr[11] = 6;
    addr[12..18].copy_from_slice(&frame[6..12]);

    let socks: Vec<Arc<PacketSocket>> = {
        let guard = PACKET_SOCKS.lock();
        guard.iter().filter_map(Weak::upgrade).collect()
    };

    for sock in socks {
        let woke = {
            let mut inner = sock.inner.lock();
            let accept = inner.ifindex.is_none_or(|want| want == ifindex)
                && (inner.protocol == ETH_P_ALL || inner.protocol == ethertype)
                && inner.recv_queue.len() < PACKET_RECV_QUEUE_LIMIT;
            if accept {
                inner.recv_queue.push_back(PacketFrame {
                    addr,
                    data: frame.to_vec(),
                });
            }
            accept
        };
        if woke {
            sock.rd_event.wake_all();
        }
    }
}
