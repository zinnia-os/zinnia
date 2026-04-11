pub type time_t = isize;
pub type suseconds_t = isize;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct timespec {
    pub tv_sec: time_t,
    pub tv_nsec: isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct timeval {
    pub tv_sec: time_t,
    pub tv_usec: suseconds_t,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct itimerval {
    pub it_interval: timeval,
    pub it_value: timeval,
}

pub const ITIMER_REAL: usize = 0;
pub const ITIMER_VIRTUAL: usize = 1;
pub const ITIMER_PROF: usize = 2;
