use crate::memory::UserPtr;

pub const POLL_IN: u32 = 1;
pub const POLL_OUT: u32 = 2;
pub const POLL_MSG: u32 = 3;
pub const POLL_ERR: u32 = 4;
pub const POLL_PRI: u32 = 5;
pub const POLL_HUP: u32 = 6;

pub const SIGABRT: u32 = 1;
pub const SIGALRM: u32 = 2;
pub const SIGBUS: u32 = 3;
pub const SIGCHLD: u32 = 4;
pub const SIGCONT: u32 = 5;
pub const SIGFPE: u32 = 6;
pub const SIGHUP: u32 = 7;
pub const SIGILL: u32 = 8;
pub const SIGINT: u32 = 9;
pub const SIGKILL: u32 = 10;
pub const SIGPIPE: u32 = 11;
pub const SIGQUIT: u32 = 12;
pub const SIGSEGV: u32 = 13;
pub const SIGSTOP: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGTSTP: u32 = 16;
pub const SIGTTIN: u32 = 17;
pub const SIGTTOU: u32 = 18;
pub const SIGUSR1: u32 = 19;
pub const SIGUSR2: u32 = 20;
pub const SIGWINCH: u32 = 21;
pub const SIGSYS: u32 = 22;
pub const SIGTRAP: u32 = 23;
pub const SIGURG: u32 = 24;
pub const SIGVTALRM: u32 = 25;
pub const SIGXCPU: u32 = 26;
pub const SIGXFSZ: u32 = 27;
pub const SIGIO: u32 = 28;
pub const SIGPOLL: u32 = 29;
pub const SIGPROF: u32 = 30;
pub const SIGPWR: u32 = 31;
pub const SIGIOT: u32 = 32;
pub const SIGCANCEL: u32 = 33;

pub const MAX_SIGNAL: u32 = 33;

pub const BUS_ADRALN: u32 = 1;
pub const BUS_ADRERR: u32 = 2;
pub const BUS_OBJERR: u32 = 3;

pub const ILL_ILLOPC: u32 = 1;
pub const ILL_ILLOPN: u32 = 2;
pub const ILL_ILLADR: u32 = 3;
pub const ILL_ILLTRP: u32 = 4;
pub const ILL_PRVOPC: u32 = 5;
pub const ILL_PRVREG: u32 = 6;
pub const ILL_COPROC: u32 = 7;
pub const ILL_BADSTK: u32 = 8;
pub const ILL_BADIADDR: u32 = 9;

pub const SEGV_MAPERR: u32 = 1;
pub const SEGV_ACCERR: u32 = 2;

pub const SIG_BLOCK: u32 = 1;
pub const SIG_UNBLOCK: u32 = 2;
pub const SIG_SETMASK: u32 = 3;

pub const SA_NOCLDSTOP: u32 = 1 << 0;
pub const SA_ONSTACK: u32 = 1 << 1;
pub const SA_RESETHAND: u32 = 1 << 2;
pub const SA_RESTART: u32 = 1 << 3;
pub const SA_SIGINFO: u32 = 1 << 4;
pub const SA_NOCLDWAIT: u32 = 1 << 5;
pub const SA_NODEFER: u32 = 1 << 6;

pub const MINSIGSTKSZ: u32 = 2048;
pub const SIGSTKSZ: u32 = 8192;
pub const SS_ONSTACK: u32 = 1;
pub const SS_DISABLE: u32 = 2;

pub const SIGEV_NONE: u32 = 1;
pub const SIGEV_SIGNAL: u32 = 2;
pub const SIGEV_THREAD: u32 = 3;

pub const SI_ASYNCNL: u32 = -60i32 as u32;
pub const SI_TKILL: u32 = -6i32 as u32;
pub const SI_SIGIO: u32 = -5i32 as u32;
pub const SI_ASYNCIO: u32 = -4i32 as u32;
pub const SI_MESGQ: u32 = -3i32 as u32;
pub const SI_TIMER: u32 = -2i32 as u32;
pub const SI_QUEUE: u32 = -1i32 as u32;
pub const SI_USER: u32 = 0;
pub const SI_KERNEL: u32 = 128;

pub const NSIG: u32 = 65;
pub const _NSIG: u32 = NSIG;

pub const CLD_EXITED: u32 = 1;
pub const CLD_KILLED: u32 = 2;
pub const CLD_DUMPED: u32 = 3;
pub const CLD_TRAPPED: u32 = 4;
pub const CLD_STOPPED: u32 = 5;
pub const CLD_CONTINUED: u32 = 6;

pub type sigset_t = u64;

/// Special handler values.
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub union sigval {
    pub sival_int: i32,
    pub sival_ptr: UserPtr<()>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct sigevent {
    pub sigev_notify: i32,
    pub sigev_signo: i32,
    pub sigev_value: sigval,
    pub sigev_notify_function: Option<extern "C" fn(sigval)>,
    pub sigev_notify_attributes: UserPtr<super::pthread_attr_t>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct siginfo_t {
    pub si_signo: i32,
    pub si_code: i32,
    pub si_errno: i32,
    pub si_pid: super::pid_t,
    pub si_uid: super::uid_t,
    pub si_addr: UserPtr<()>,
    pub si_status: i32,
    pub si_value: sigval,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct stack_t {
    pub ss_sp: UserPtr<()>,
    pub ss_size: usize,
    pub ss_flags: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct sigaction {
    pub sa_handler: usize,
    pub sa_flags: u64,
    pub sa_restorer: usize,
    pub sa_mask: sigset_t,
}
