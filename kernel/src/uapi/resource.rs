pub const RUSAGE_SELF: u32 = 0;
pub const RUSAGE_CHILDREN: u32 = u32::MAX;

pub const RLIMIT_CPU: u32 = 0;
pub const RLIMIT_FSIZE: u32 = 1;
pub const RLIMIT_DATA: u32 = 2;
pub const RLIMIT_STACK: u32 = 3;
pub const RLIMIT_CORE: u32 = 4;
pub const RLIMIT_RSS: u32 = 5;
pub const RLIMIT_NPROC: u32 = 6;
pub const RLIMIT_NOFILE: u32 = 7;
pub const RLIMIT_MEMLOCK: u32 = 8;
pub const RLIMIT_AS: u32 = 9;
pub const RLIMIT_LOCKS: u32 = 10;
pub const RLIMIT_SIGPENDING: u32 = 11;
pub const RLIMIT_MSGQUEUE: u32 = 12;
pub const RLIMIT_NICE: u32 = 13;
pub const RLIMIT_RTPRIO: u32 = 14;
pub const RLIMIT_RTTIME: u32 = 15;
pub const RLIMIT_NLIMITS: u32 = 16;

pub const PRIO_PROCESS: u32 = 0;
pub const PRIO_PGRP: u32 = 1;
pub const PRIO_USER: u32 = 2;

pub const PRIO_MIN: u32 = -20i32 as u32;
pub const PRIO_MAX: u32 = 20;

pub const RLIM_INFINITY: super::rlim_t = -1i32 as super::rlim_t;
pub const RLIM_SAVED_MAX: super::rlim_t = -1i32 as super::rlim_t;
pub const RLIM_SAVED_CUR: super::rlim_t = -1i32 as super::rlim_t;

pub const RLIM_NLIMITS: u32 = RLIMIT_NLIMITS;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct rusage {
    pub ru_utime: super::time::timeval,
    pub ru_stime: super::time::timeval,
    pub ru_maxrss: isize,
    pub ru_ixrss: isize,
    pub ru_idrss: isize,
    pub ru_isrss: isize,
    pub ru_minflt: isize,
    pub ru_majflt: isize,
    pub ru_nswap: isize,
    pub ru_inblock: isize,
    pub ru_oublock: isize,
    pub ru_msgsnd: isize,
    pub ru_msgrcv: isize,
    pub ru_nsignals: isize,
    pub ru_nvcsw: isize,
    pub ru_nivcsw: isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct rlimit {
    pub rlim_cur: super::rlim_t,
    pub rlim_max: super::rlim_t,
}
