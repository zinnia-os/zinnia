pub const EPOLLIN: u32 = 1 << 0;
pub const EPOLLOUT: u32 = 1 << 1;
pub const EPOLLPRI: u32 = 1 << 2;
pub const EPOLLHUP: u32 = 1 << 3;
pub const EPOLLERR: u32 = 1 << 4;
pub const EPOLLRDHUP: u32 = 1 << 5;
pub const EPOLLMSG: u32 = 1 << 6;
pub const EPOLLRDNORM: u32 = 1 << 7;
pub const EPOLLRDBAND: u32 = 1 << 8;
pub const EPOLLWRNORM: u32 = 1 << 9;
pub const EPOLLWRBAND: u32 = 1 << 10;
pub const EPOLLEXCLUSIVE: u32 = 1 << 11;
pub const EPOLLWAKEUP: u32 = 1 << 12;
pub const EPOLLONESHOT: u32 = 1 << 13;
pub const EPOLLET: u32 = 1 << 14;

pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub union epoll_data {
    pub ptr: usize,
    pub fd: i32,
    pub num_u32: u32,
    pub num_u64: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct epoll_event {
    pub events: u32,
    pub data: epoll_data,
}
