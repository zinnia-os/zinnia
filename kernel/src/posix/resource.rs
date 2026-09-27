use crate::uapi::resource::*;

#[repr(u32)]
pub enum Resource {
    Cpu = RLIMIT_CPU,
    FileSize = RLIMIT_FSIZE,
    Data = RLIMIT_DATA,
    Stack = RLIMIT_STACK,
    Core = RLIMIT_CORE,
    Rss = RLIMIT_RSS,
    NumProc = RLIMIT_NPROC,
    NumFiles = RLIMIT_NOFILE,
    MemLock = RLIMIT_MEMLOCK,
    AddressSpace = RLIMIT_AS,
    Locks = RLIMIT_LOCKS,
    SigPending = RLIMIT_SIGPENDING,
    MsgQueue = RLIMIT_MSGQUEUE,
    Nice = RLIMIT_NICE,
    RtPrio = RLIMIT_RTPRIO,
    RtTime = RLIMIT_RTTIME,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub open_max: rlimit,
    pub core_size: rlimit,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            open_max: rlimit {
                rlim_cur: 1024,
                rlim_max: 1024,
            },
            core_size: rlimit {
                rlim_cur: 0,
                rlim_max: RLIM_INFINITY,
            },
        }
    }
}
