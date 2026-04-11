pub const POLLIN: i16 = 0x01;
pub const POLLOUT: i16 = 0x02;
pub const POLLPRI: i16 = 0x04;
pub const POLLHUP: i16 = 0x08;
pub const POLLERR: i16 = 0x10;
pub const POLLRDHUP: i16 = 0x20;
pub const POLLNVAL: i16 = 0x40;
pub const POLLRDNORM: i16 = 0x80;
pub const POLLRDBAND: i16 = 0x100;
pub const POLLWRNORM: i16 = 0x200;
pub const POLLWRBAND: i16 = 0x400;

pub type nfds_t = isize;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct pollfd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}
