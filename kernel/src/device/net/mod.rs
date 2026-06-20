use crate::{
    device::net::l3::ipv4::Ipv4Addr,
    memory::{IovecIter, VirtAddr, user::UserPtr},
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

pub mod dev;
pub mod interface;
pub mod l2;
pub mod l3;
pub mod l4;
pub mod local;
pub mod nic;

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
        addr: Option<&[u8]>,
        control: &[u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<isize> {
        let _ = (addr, control);
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
        addr: Option<&mut [u8]>,
        control: &mut [u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<(isize, usize, usize, u32)> {
        let _ = (addr, control);
        let n = self.recv(buf, flags, nonblocking)?;
        Ok((n, 0, 0, 0))
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

    /// Called when the last file descriptor for this open socket is dropped.
    fn on_close(&self) {}
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.ops.on_close();
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

    fn ioctl(&self, _file: &File, request: usize, argp: VirtAddr) -> EResult<usize> {
        use uapi::net::*;

        let request = request as u32;
        match request {
            SIOCGIFCONF => {
                let mut ptr: UserPtr<ifconf> = UserPtr::new(argp);
                let mut conf = ptr.read().ok_or(Errno::EFAULT)?;
                let interfaces = interface::snapshot();
                let entry = size_of::<ifreq>();

                if conf.ifc_buf == 0 {
                    conf.ifc_len = (interfaces.len() * entry) as i32;
                    ptr.write(conf).ok_or(Errno::EFAULT)?;
                    return Ok(0);
                }

                let max = conf.ifc_len as usize / entry;
                let mut out: UserPtr<ifreq> = UserPtr::new(VirtAddr::new(conf.ifc_buf as usize));
                let mut written = 0;
                for iface in interfaces.iter().take(max) {
                    let mut req = ifreq {
                        ifr_name: *iface.name(),
                        ifr_ifru: [0; 24],
                    };
                    write_sockaddr_in(&mut req.ifr_ifru, iface.ip());
                    out.write(req).ok_or(Errno::EFAULT)?;
                    out = out.offset(1);
                    written += 1;
                }
                conf.ifc_len = (written * entry) as i32;
                ptr.write(conf).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            SIOCGIFINDEX | SIOCGIFHWADDR | SIOCGIFADDR | SIOCGIFNETMASK | SIOCGIFFLAGS => {
                let mut ptr: UserPtr<ifreq> = UserPtr::new(argp);
                let mut req = ptr.read().ok_or(Errno::EFAULT)?;
                let iface = interface::by_name(&req.ifr_name).ok_or(Errno::ENODEV)?;
                match request {
                    SIOCGIFINDEX => {
                        req.ifr_ifru[..4].copy_from_slice(&(iface.index() as i32).to_ne_bytes())
                    }
                    SIOCGIFHWADDR => {
                        req.ifr_ifru = [0; 24];
                        req.ifr_ifru[..2].copy_from_slice(&ARPHRD_ETHER.to_ne_bytes());
                        req.ifr_ifru[2..8].copy_from_slice(iface.mac().as_bytes());
                    }
                    SIOCGIFADDR => write_sockaddr_in(&mut req.ifr_ifru, iface.ip()),
                    SIOCGIFNETMASK => write_sockaddr_in(&mut req.ifr_ifru, iface.netmask()),
                    SIOCGIFFLAGS => req.ifr_ifru[..2].copy_from_slice(&iface.flags().to_ne_bytes()),
                    _ => unreachable!(),
                }
                ptr.write(req).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            SIOCSIFADDR | SIOCSIFNETMASK | SIOCSIFFLAGS => {
                let ptr: UserPtr<ifreq> = UserPtr::new(argp);
                let req = ptr.read().ok_or(Errno::EFAULT)?;
                let name = ifr_name_str(&req.ifr_name);
                let Some(iface) = interface::by_name(&req.ifr_name) else {
                    log!("SIOC config for unknown interface {:?}", name);
                    return Err(Errno::ENODEV);
                };
                match request {
                    SIOCSIFADDR => {
                        let ip = read_sockaddr_in(&req.ifr_ifru);
                        log!("{} address {}", name, ip);
                        iface.set_ip(ip);
                    }
                    SIOCSIFNETMASK => {
                        let mask = read_sockaddr_in(&req.ifr_ifru);
                        log!("{} netmask {}", name, mask);
                        iface.set_netmask(mask);
                    }
                    SIOCSIFFLAGS => {
                        let flags = i16::from_ne_bytes([req.ifr_ifru[0], req.ifr_ifru[1]]);
                        log!("{} flags {:#06x}", name, flags);
                        iface.set_flags(flags);
                    }
                    _ => unreachable!(),
                }
                Ok(0)
            }
            SIOCADDRT | SIOCDELRT => {
                let ptr: UserPtr<rtentry> = UserPtr::new(argp);
                let rt = ptr.read().ok_or(Errno::EFAULT)?;
                let iface = interface::default_ipv4_interface().ok_or(Errno::ENODEV)?;
                let name = ifr_name_str(iface.name());
                if request == SIOCDELRT {
                    log!("{} drop default route", name);
                    iface.set_gateway(None);
                } else {
                    let gw = Ipv4Addr::new([
                        rt.rt_gateway.sa_data[2],
                        rt.rt_gateway.sa_data[3],
                        rt.rt_gateway.sa_data[4],
                        rt.rt_gateway.sa_data[5],
                    ]);
                    log!("{} default route via {}", name, gw);
                    iface.set_gateway((gw != Ipv4Addr::ANY).then_some(gw));
                }
                Ok(0)
            }
            _ => Err(Errno::ENOTTY),
        }
    }
}

fn write_sockaddr_in(buf: &mut [u8; 24], addr: Ipv4Addr) {
    *buf = [0; 24];
    buf[..2].copy_from_slice(&(uapi::socket::AF_INET as u16).to_ne_bytes());
    buf[4..8].copy_from_slice(addr.as_bytes());
}

fn read_sockaddr_in(buf: &[u8; 24]) -> Ipv4Addr {
    Ipv4Addr::new([buf[4], buf[5], buf[6], buf[7]])
}

fn ifr_name_str(name: &[u8]) -> &str {
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    core::str::from_utf8(&name[..end]).unwrap_or("?")
}
