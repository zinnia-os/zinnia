use super::socket::sockaddr;

pub const IFNAMSIZ: usize = 16;

pub const SIOCGIFCONF: u32 = 0x8912;
pub const SIOCGIFFLAGS: u32 = 0x8913;
pub const SIOCSIFFLAGS: u32 = 0x8914;
pub const SIOCGIFADDR: u32 = 0x8915;
pub const SIOCSIFADDR: u32 = 0x8916;
pub const SIOCGIFNETMASK: u32 = 0x891b;
pub const SIOCSIFNETMASK: u32 = 0x891c;
pub const SIOCGIFHWADDR: u32 = 0x8927;
pub const SIOCGIFINDEX: u32 = 0x8933;
pub const SIOCADDRT: u32 = 0x890b;
pub const SIOCDELRT: u32 = 0x890c;

pub const IFF_UP: i16 = 0x1;
pub const IFF_BROADCAST: i16 = 0x2;
pub const IFF_RUNNING: i16 = 0x40;
pub const IFF_MULTICAST: i16 = 0x1000;

pub const ARPHRD_ETHER: u16 = 1;

pub const RTF_UP: u16 = 0x0001;
pub const RTF_GATEWAY: u16 = 0x0002;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ifreq {
    pub ifr_name: [u8; IFNAMSIZ],
    pub ifr_ifru: [u8; 24],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ifconf {
    pub ifc_len: i32,
    _pad: u32,
    pub ifc_buf: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct rtentry {
    pub rt_pad1: usize,
    pub rt_dst: sockaddr,
    pub rt_gateway: sockaddr,
    pub rt_genmask: sockaddr,
    pub rt_flags: u16,
    pub rt_pad2: i16,
    pub rt_pad3: usize,
    pub rt_tos: u8,
    pub rt_class: u8,
    pub rt_pad4: [i16; 3],
    pub rt_metric: i16,
    pub rt_dev: usize,
    pub rt_mtu: usize,
    pub rt_window: usize,
    pub rt_irtt: u16,
}
