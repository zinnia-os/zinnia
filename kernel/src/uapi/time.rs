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

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct itimerspec {
    pub it_interval: timespec,
    pub it_value: timespec,
}

pub const ITIMER_REAL: usize = 0;
pub const ITIMER_VIRTUAL: usize = 1;
pub const ITIMER_PROF: usize = 2;

pub const CLOCK_REALTIME: usize = 0;
pub const CLOCK_MONOTONIC: usize = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: usize = 2;
pub const CLOCK_THREAD_CPUTIME_ID: usize = 3;
pub const CLOCK_MONOTONIC_RAW: usize = 4;
pub const CLOCK_REALTIME_COARSE: usize = 5;
pub const CLOCK_MONOTONIC_COARSE: usize = 6;
pub const CLOCK_BOOTTIME: usize = 7;

pub const TFD_CLOEXEC: u32 = crate::uapi::fcntl::O_CLOEXEC;
pub const TFD_NONBLOCK: u32 = crate::uapi::fcntl::O_NONBLOCK;
pub const TFD_TIMER_ABSTIME: i32 = 1;
