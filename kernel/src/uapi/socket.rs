use super::uio::iovec;
use crate::memory::UserPtr;

pub type socklen_t = u32;
pub type sa_family_t = u16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct msghdr {
    pub msg_name: UserPtr<()>,
    pub msg_namelen: socklen_t,
    pub msg_iov: UserPtr<iovec>,
    pub msg_iovlen: i32,
    pub msg_control: UserPtr<()>,
    pub msg_controllen: socklen_t,
    pub msg_flags: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct mmsghdr {
    pub msg_hdr: msghdr,
    pub msg_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct cmsghdr {
    pub cmsg_len: socklen_t,
    pub cmsg_level: i32,
    pub cmsg_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct sockaddr_storage {
    pub s_family: sa_family_t,
    padding: [u8; 128 - size_of::<sa_family_t>() - size_of::<usize>()],
    alignment: usize,
}

static_assert!(size_of::<sockaddr_storage>() == 128);
static_assert!(align_of::<sockaddr_storage>() == size_of::<usize>());

#[repr(C)]
#[derive(Clone, Copy)]
pub struct sockaddr {
    pub sa_family: sa_family_t,
    pub sa_data: [u8; 14],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct sockaddr_un {
    pub sun_family: sa_family_t,
    pub sun_path: [u8; 108],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ucred {
    pub pid: super::pid_t,
    pub uid: super::uid_t,
    pub gid: super::gid_t,
}

pub const SCM_RIGHTS: u32 = 1;
pub const SCM_TIMESTAMP: u32 = SO_TIMESTAMP;
pub const SCM_TIMESTAMPNS: u32 = SO_TIMESTAMPNS;

/*MISSING: CMSG_DATA, CMSG_NXTHDR, CMSG_FIRSTHDR */

pub const SCM_CREDENTIALS: u32 = 0x02;

pub const SOCK_DGRAM: u32 = 1 << 0;
pub const SOCK_RAW: u32 = 1 << 1;
pub const SOCK_STREAM: u32 = 1 << 2;
pub const SOCK_SEQPACKET: u32 = 1 << 3;
pub const SOCK_NONBLOCK: u32 = 1 << 15;
pub const SOCK_CLOEXEC: u32 = 1 << 16;
pub const SOCK_CLOFORK: u32 = 1 << 17;

pub const SOL_SOCKET: u32 = 1;
pub const SOL_IPV6: u32 = 41;
pub const SOL_PACKET: u32 = 263;
pub const SOL_NETLINK: u32 = 270;

pub const SO_ACCEPTCONN: u32 = 1;
pub const SO_BROADCAST: u32 = 2;
pub const SO_DEBUG: u32 = 3;
pub const SO_DONTROUTE: u32 = 4;
pub const SO_ERROR: u32 = 5;
pub const SO_KEEPALIVE: u32 = 6;
pub const SO_LINGER: u32 = 7;
pub const SO_OOBINLINE: u32 = 8;
pub const SO_RCVBUF: u32 = 9;
pub const SO_RCVLOWAT: u32 = 10;
pub const SO_RCVTIMEO: u32 = 11;
pub const SO_REUSEADDR: u32 = 12;
pub const SO_SNDBUF: u32 = 13;
pub const SO_SNDLOWAT: u32 = 14;
pub const SO_SNDTIMEO: u32 = 15;
pub const SO_TYPE: u32 = 16;
pub const SO_SNDBUFFORCE: u32 = 17;
pub const SO_PEERCRED: u32 = 18;
pub const SO_ATTACH_FILTER: u32 = 19;
pub const SO_PASSCRED: u32 = 20;
pub const SO_RCVBUFFORCE: u32 = 21;
pub const SO_DETACH_FILTER: u32 = 22;
pub const SO_PROTOCOL: u32 = 23;
pub const SO_REUSEPORT: u32 = 24;
pub const SO_TIMESTAMP: u32 = 25;
pub const SO_PEERSEC: u32 = 26;
pub const SO_BINDTODEVICE: u32 = 27;
pub const SO_DOMAIN: u32 = 28;
pub const SO_PASSSEC: u32 = 29;
pub const SO_TIMESTAMPNS: u32 = 30;
pub const SO_PRIORITY: u32 = 31;
pub const SO_MARK: u32 = 32;

pub const SOMAXCONN: u32 = 1;

pub const MSG_CTRUNC: u32 = 0x1;
pub const MSG_DONTROUTE: u32 = 0x2;
pub const MSG_EOR: u32 = 0x4;
pub const MSG_OOB: u32 = 0x8;
pub const MSG_NOSIGNAL: u32 = 0x10;
pub const MSG_PEEK: u32 = 0x20;
pub const MSG_TRUNC: u32 = 0x40;
pub const MSG_WAITALL: u32 = 0x80;
pub const MSG_FIN: u32 = 0x200;
pub const MSG_CONFIRM: u32 = 0x800;

/* Linux extensions. */
pub const MSG_DONTWAIT: u32 = 0x1000;
pub const MSG_CMSG_CLOEXEC: u32 = 0x2000;
pub const MSG_MORE: u32 = 0x4000;
pub const MSG_FASTOPEN: u32 = 0x20000000;

/* GNU (?) extension: Protocol family constants. */

pub const PF_INET: u32 = 1;
pub const PF_INET6: u32 = 2;
pub const PF_UNIX: u32 = 3;
pub const PF_LOCAL: u32 = 3;
pub const PF_UNSPEC: u32 = 4;
pub const PF_NETLINK: u32 = 5;
pub const PF_BRIDGE: u32 = 6;
pub const PF_APPLETALK: u32 = 7;
pub const PF_BLUETOOTH: u32 = 8;
pub const PF_DECNET: u32 = 9;
pub const PF_IPX: u32 = 10;
pub const PF_ISDN: u32 = 11;
pub const PF_SNA: u32 = 12;
pub const PF_PACKET: u32 = 13;
pub const PF_AX25: u32 = 14;
pub const PF_NETROM: u32 = 15;
pub const PF_ROSE: u32 = 16;
pub const PF_TIPC: u32 = 30;
pub const PF_ALG: u32 = 38;
pub const PF_MAX: u32 = 46;

pub const AF_INET: u32 = PF_INET;
pub const AF_INET6: u32 = PF_INET6;
pub const AF_UNIX: u32 = PF_UNIX;
pub const AF_LOCAL: u32 = PF_LOCAL;
pub const AF_UNSPEC: u32 = PF_UNSPEC;
pub const AF_NETLINK: u32 = PF_NETLINK;
pub const AF_BRIDGE: u32 = PF_BRIDGE;
pub const AF_APPLETALK: u32 = PF_APPLETALK;
pub const AF_BLUETOOTH: u32 = PF_BLUETOOTH;
pub const AF_DECNET: u32 = PF_DECNET;
pub const AF_IPX: u32 = PF_IPX;
pub const AF_ISDN: u32 = PF_ISDN;
pub const AF_SNA: u32 = PF_SNA;
pub const AF_PACKET: u32 = PF_PACKET;
pub const AF_AX25: u32 = PF_AX25;
pub const AF_NETROM: u32 = PF_NETROM;
pub const AF_ROSE: u32 = PF_ROSE;
pub const AF_TIPC: u32 = PF_TIPC;
pub const AF_ALG: u32 = PF_ALG;
pub const AF_MAX: u32 = PF_MAX;

pub const SHUT_RD: u32 = 1;
pub const SHUT_RDWR: u32 = 2;
pub const SHUT_WR: u32 = 3;
