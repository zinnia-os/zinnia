use crate::uapi::signal;

#[repr(u32)]
pub enum Signal {
    SIGHUP = signal::SIGHUP,
    SIGINT = signal::SIGINT,
    SIGQUIT = signal::SIGQUIT,
    SIGCONT = signal::SIGCONT,
    SIGBUS = signal::SIGBUS,
    SIGABRT = signal::SIGABRT,
    SIGCHLD = signal::SIGCHLD,
    SIGFPE = signal::SIGFPE,
    SIGKILL = signal::SIGKILL,
    SIGILL = signal::SIGILL,
    SIGPIPE = signal::SIGPIPE,
    SIGSEGV = signal::SIGSEGV,
    SIGSTOP = signal::SIGSTOP,
    SIGALRM = signal::SIGALRM,
    SIGTERM = signal::SIGTERM,
    SIGTSTP = signal::SIGTSTP,
    SIGTTIN = signal::SIGTTIN,
    SIGTTOU = signal::SIGTTOU,
    SIGUSR1 = signal::SIGUSR1,
    SIGUSR2 = signal::SIGUSR2,
    SIGIO = signal::SIGIO,
    SIGPROF = signal::SIGPROF,
    SIGSYS = signal::SIGSYS,
    SIGTRAP = signal::SIGTRAP,
    SIGURG = signal::SIGURG,
    SIGVTALRM = signal::SIGVTALRM,
    SIGXCPU = signal::SIGXCPU,
    SIGXFSZ = signal::SIGXFSZ,
    SIGWINCH = signal::SIGWINCH,
    SIGPWR = signal::SIGPWR,
}

/// Wrapper around a set of signals.
#[repr(transparent)]
pub struct SignalSet {
    inner: signal::sigset_t,
}

impl SignalSet {
    pub fn new() -> Self {
        Self { inner: 0 }
    }

    pub fn set_signal(&mut self, idx: Signal, state: bool) {
        if state {
            self.inner |= 1 << idx as u32;
        } else {
            self.inner &= !(1 << idx as u32);
        }
    }
}
