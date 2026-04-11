use crate::{
    arch::sched::Context,
    process::Process,
    sched::Scheduler,
    uapi::signal::{self, MAX_SIGNAL, SIG_DFL, SIG_IGN, sigaction},
};
use core::ops;

/// POSIX signals represented as an enum.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    SIGABRT = signal::SIGABRT,
    SIGALRM = signal::SIGALRM,
    SIGBUS = signal::SIGBUS,
    SIGCHLD = signal::SIGCHLD,
    SIGCONT = signal::SIGCONT,
    SIGFPE = signal::SIGFPE,
    SIGHUP = signal::SIGHUP,
    SIGILL = signal::SIGILL,
    SIGINT = signal::SIGINT,
    SIGKILL = signal::SIGKILL,
    SIGPIPE = signal::SIGPIPE,
    SIGQUIT = signal::SIGQUIT,
    SIGSEGV = signal::SIGSEGV,
    SIGSTOP = signal::SIGSTOP,
    SIGTERM = signal::SIGTERM,
    SIGTSTP = signal::SIGTSTP,
    SIGTTIN = signal::SIGTTIN,
    SIGTTOU = signal::SIGTTOU,
    SIGUSR1 = signal::SIGUSR1,
    SIGUSR2 = signal::SIGUSR2,
    SIGWINCH = signal::SIGWINCH,
    SIGSYS = signal::SIGSYS,
    SIGTRAP = signal::SIGTRAP,
    SIGURG = signal::SIGURG,
    SIGVTALRM = signal::SIGVTALRM,
    SIGXCPU = signal::SIGXCPU,
    SIGXFSZ = signal::SIGXFSZ,
    SIGIO = signal::SIGIO,
    SIGPOLL = signal::SIGPOLL,
    SIGPROF = signal::SIGPROF,
    SIGPWR = signal::SIGPWR,
    SIGIOT = signal::SIGIOT,
    SIGCANCEL = signal::SIGCANCEL,
}

impl Signal {
    /// Try to convert a raw signal number to a [`Signal`].
    pub fn from_raw(num: u32) -> Option<Self> {
        match num {
            signal::SIGABRT => Some(Self::SIGABRT),
            signal::SIGALRM => Some(Self::SIGALRM),
            signal::SIGBUS => Some(Self::SIGBUS),
            signal::SIGCHLD => Some(Self::SIGCHLD),
            signal::SIGCONT => Some(Self::SIGCONT),
            signal::SIGFPE => Some(Self::SIGFPE),
            signal::SIGHUP => Some(Self::SIGHUP),
            signal::SIGILL => Some(Self::SIGILL),
            signal::SIGINT => Some(Self::SIGINT),
            signal::SIGKILL => Some(Self::SIGKILL),
            signal::SIGPIPE => Some(Self::SIGPIPE),
            signal::SIGQUIT => Some(Self::SIGQUIT),
            signal::SIGSEGV => Some(Self::SIGSEGV),
            signal::SIGSTOP => Some(Self::SIGSTOP),
            signal::SIGTERM => Some(Self::SIGTERM),
            signal::SIGTSTP => Some(Self::SIGTSTP),
            signal::SIGTTIN => Some(Self::SIGTTIN),
            signal::SIGTTOU => Some(Self::SIGTTOU),
            signal::SIGUSR1 => Some(Self::SIGUSR1),
            signal::SIGUSR2 => Some(Self::SIGUSR2),
            signal::SIGWINCH => Some(Self::SIGWINCH),
            signal::SIGSYS => Some(Self::SIGSYS),
            signal::SIGTRAP => Some(Self::SIGTRAP),
            signal::SIGURG => Some(Self::SIGURG),
            signal::SIGVTALRM => Some(Self::SIGVTALRM),
            signal::SIGXCPU => Some(Self::SIGXCPU),
            signal::SIGXFSZ => Some(Self::SIGXFSZ),
            signal::SIGIO => Some(Self::SIGIO),
            signal::SIGPOLL => Some(Self::SIGPOLL),
            signal::SIGPROF => Some(Self::SIGPROF),
            signal::SIGPWR => Some(Self::SIGPWR),
            signal::SIGIOT => Some(Self::SIGIOT),
            signal::SIGCANCEL => Some(Self::SIGCANCEL),
            _ => None,
        }
    }

    pub fn as_raw(self) -> u32 {
        self as u32
    }

    /// Returns the default action for this signal.
    pub fn default_action(self) -> DefaultAction {
        match self {
            Signal::SIGHUP
            | Signal::SIGINT
            | Signal::SIGPIPE
            | Signal::SIGALRM
            | Signal::SIGTERM
            | Signal::SIGUSR1
            | Signal::SIGUSR2
            | Signal::SIGPROF
            | Signal::SIGVTALRM
            | Signal::SIGIO
            | Signal::SIGPOLL
            | Signal::SIGPWR => DefaultAction::Terminate,
            Signal::SIGQUIT
            | Signal::SIGABRT
            | Signal::SIGBUS
            | Signal::SIGFPE
            | Signal::SIGILL
            | Signal::SIGSEGV
            | Signal::SIGSYS
            | Signal::SIGTRAP
            | Signal::SIGXCPU
            | Signal::SIGXFSZ
            | Signal::SIGIOT => DefaultAction::CoreDump,
            Signal::SIGSTOP | Signal::SIGTSTP | Signal::SIGTTIN | Signal::SIGTTOU => {
                DefaultAction::Stop
            }
            Signal::SIGCONT => DefaultAction::Continue,
            Signal::SIGCHLD | Signal::SIGURG | Signal::SIGWINCH => DefaultAction::Ignore,
            Signal::SIGKILL | Signal::SIGCANCEL => DefaultAction::Terminate,
        }
    }

    /// Returns true if this signal cannot be caught or ignored.
    pub fn is_uncatchable(self) -> bool {
        matches!(self, Signal::SIGKILL | Signal::SIGSTOP)
    }
}

/// The default kernel-side action for a signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAction {
    Terminate,
    CoreDump,
    Stop,
    Continue,
    Ignore,
}

/// Wrapper around a set of signals, stored as a bitmask.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SignalSet {
    inner: signal::sigset_t,
}

/// Signals that can never be blocked.
const UNBLOCKABLE: u64 = (1u64 << signal::SIGKILL) | (1u64 << signal::SIGSTOP);

impl SignalSet {
    pub const fn new() -> Self {
        Self { inner: 0 }
    }

    pub const fn from_raw(raw: signal::sigset_t) -> Self {
        Self { inner: raw }
    }

    pub const fn as_raw(self) -> signal::sigset_t {
        self.inner
    }

    pub fn set(&mut self, sig: Signal, state: bool) {
        let bit = 1u64 << sig.as_raw();
        if state {
            self.inner |= bit;
        } else {
            self.inner &= !bit;
        }
    }

    pub fn is_set(&self, sig: Signal) -> bool {
        self.inner & (1u64 << sig.as_raw()) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.inner == 0
    }

    /// Returns the lowest signal number that is set, or None.
    pub fn first_set(&self) -> Option<Signal> {
        if self.inner == 0 {
            return None;
        }
        let bit = self.inner.trailing_zeros();
        Signal::from_raw(bit)
    }

    /// Remove the unblockable signals (SIGKILL, SIGSTOP) from this set.
    pub fn sanitize_mask(&mut self) {
        self.inner &= !UNBLOCKABLE;
    }
}

impl ops::BitOr for SignalSet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self {
            inner: self.inner | rhs.inner,
        }
    }
}

impl ops::BitAnd for SignalSet {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self {
            inner: self.inner & rhs.inner,
        }
    }
}

impl ops::Not for SignalSet {
    type Output = Self;
    fn not(self) -> Self {
        Self { inner: !self.inner }
    }
}

impl ops::BitOrAssign for SignalSet {
    fn bitor_assign(&mut self, rhs: Self) {
        self.inner |= rhs.inner;
    }
}

impl ops::BitAndAssign for SignalSet {
    fn bitand_assign(&mut self, rhs: Self) {
        self.inner &= rhs.inner;
    }
}

/// A kernel-internal representation of a signal action.
#[derive(Clone, Copy, Debug)]
pub struct SigAction {
    /// The handler address: SIG_DFL (0), SIG_IGN (1), or a user function pointer.
    pub handler: usize,
    /// Signal mask to apply during handler execution.
    pub mask: SignalSet,
    /// SA_* flags.
    pub flags: u32,
    /// The restorer function address (used for sigreturn trampoline).
    pub restorer: usize,
}

impl SigAction {
    pub const fn default() -> Self {
        Self {
            handler: SIG_DFL,
            mask: SignalSet::new(),
            flags: 0,
            restorer: 0,
        }
    }

    pub fn is_default(&self) -> bool {
        self.handler == SIG_DFL
    }

    pub fn is_ignore(&self) -> bool {
        self.handler == SIG_IGN
    }

    /// Convert from the userspace ABI structure.
    pub fn from_user(u: &sigaction) -> Self {
        Self {
            handler: u.sa_handler,
            mask: SignalSet::from_raw(u.sa_mask),
            flags: u.sa_flags as u32,
            restorer: u.sa_restorer,
        }
    }

    /// Convert to the userspace ABI structure.
    pub fn to_user(&self) -> sigaction {
        sigaction {
            sa_handler: self.handler,
            sa_mask: self.mask.as_raw(),
            sa_flags: self.flags as u64,
            sa_restorer: self.restorer,
        }
    }
}

/// Per-process signal action table.
#[derive(Clone, Debug)]
pub struct SignalState {
    /// Signal actions indexed by signal number (1-based, index 0 unused).
    actions: [SigAction; (MAX_SIGNAL + 1) as usize],
}

impl SignalState {
    pub fn new() -> Self {
        Self {
            actions: [SigAction::default(); (MAX_SIGNAL + 1) as usize],
        }
    }

    pub fn get_action(&self, sig: Signal) -> &SigAction {
        &self.actions[sig.as_raw() as usize]
    }

    pub fn set_action(&mut self, sig: Signal, action: SigAction) {
        self.actions[sig.as_raw() as usize] = action;
    }

    /// Reset all caught signal handlers to SIG_DFL (for execve).
    /// SIG_IGN dispositions are preserved per POSIX.
    pub fn reset_on_exec(&mut self) {
        for i in 1..=MAX_SIGNAL as usize {
            if self.actions[i].handler != SIG_IGN {
                self.actions[i] = SigAction::default();
            }
        }
    }
}

/// Per-thread signal state.
#[derive(Clone, Debug, Default)]
pub struct ThreadSignalState {
    /// Signals pending delivery to this thread.
    pub pending: SignalSet,
    /// Current signal mask (signals blocked from delivery).
    pub mask: SignalSet,
}

/// Queue a signal on the given thread and wake it if it is sleeping.
pub fn send_signal_to_thread(task: &alloc::sync::Arc<crate::process::task::Task>, sig: Signal) {
    {
        let mut state = task.signal.lock();
        state.pending.set(sig, true);
    }
    // Wake the task so it can process the signal. If it's already on the
    // run queue this is harmless — the scheduler will just see it twice
    // and the second pick will find it Dead/not-Ready and skip it.
    crate::percpu::CpuData::get()
        .scheduler
        .add_task(task.clone());
}

/// Force-deliver a synchronous signal (e.g., from a hardware fault like SIGSEGV/SIGBUS/SIGFPE).
/// This unmasks the signal and resets its handler to SIG_DFL, ensuring it cannot be blocked
/// or caught in a way that causes an infinite fault loop.
pub fn force_signal_to_thread(task: &alloc::sync::Arc<crate::process::task::Task>, sig: Signal) {
    // Unmask the signal so it's deliverable.
    {
        let mut state = task.signal.lock();
        state.mask.set(sig, false);
        state.pending.set(sig, true);
    }

    // Reset the handler to SIG_DFL so the default action (terminate) is taken.
    let proc = task.get_process();
    proc.signal_actions
        .lock()
        .set_action(sig, SigAction::default());
}

/// Send a signal to every process in the given process group.
pub fn send_signal_to_pgrp(pgrp: crate::process::Pid, sig: Signal) {
    let table = crate::process::PROCESS_TABLE.lock();
    for proc in table.values() {
        let Some(proc) = proc.upgrade() else { continue }; // TODO: Should entries be removed?
        if *proc.pgrp.lock() == pgrp {
            // Send to the first thread of each matching process.
            let threads = proc.threads.lock();
            if let Some(t) = threads.first() {
                send_signal_to_thread(t, sig);
            }
        }
    }
}

/// Deliver pending signals to the current thread. Called before returning to userspace.
/// This may modify the context to redirect execution to a signal handler.
pub fn deliver_pending_signals(context: &mut Context) {
    loop {
        let task = Scheduler::get_current();
        let proc = task.get_process();

        let deliverable = {
            let sig_state = task.signal.lock();
            let pending = sig_state.pending;
            let mask = sig_state.mask;
            // Deliverable = pending & ~mask
            pending & !mask
        };

        if deliverable.is_empty() {
            return;
        }

        let sig = match deliverable.first_set() {
            Some(s) => s,
            None => return,
        };

        log!("Delivering signal: {:?}", sig);

        // Clear this signal from pending.
        task.signal.lock().pending.set(sig, false);

        let action = {
            let actions = proc.signal_actions.lock();
            *actions.get_action(sig)
        };

        if action.is_default() {
            match sig.default_action() {
                DefaultAction::Terminate | DefaultAction::CoreDump => {
                    // Terminate the process with exit code 128 + signal number (POSIX convention).
                    Process::exit((128 + sig.as_raw()) as u8);
                    // exit() never returns.
                }
                DefaultAction::Stop => {
                    // TODO: implement process stopping (SIGSTOP/SIGTSTP).
                    continue;
                }
                DefaultAction::Continue => {
                    // TODO: implement process continuation.
                    continue;
                }
                DefaultAction::Ignore => {
                    continue;
                }
            }
        } else if action.is_ignore() {
            continue;
        }

        // Custom handler: set up a signal frame on the user stack.
        // Save the current mask before modifying it (will be restored on sigreturn).
        let old_mask = {
            let sig_state = task.signal.lock();
            sig_state.mask
        };

        // Block the signal being delivered (and the action's mask) during handler execution.
        {
            let mut sig_state = task.signal.lock();
            if action.flags & signal::SA_NODEFER == 0 {
                sig_state.mask.set(sig, true);
            }
            sig_state.mask |= action.mask;
            sig_state.mask.sanitize_mask();
        }

        // If SA_RESETHAND, reset handler to SIG_DFL.
        if action.flags & signal::SA_RESETHAND != 0 {
            proc.signal_actions
                .lock()
                .set_action(sig, SigAction::default());
        }

        // Architecture-specific: set up the signal frame on the user stack.
        crate::arch::sched::setup_signal_frame(
            context,
            action.handler,
            sig.as_raw(),
            old_mask,
            action.restorer,
        );

        // Only deliver one signal per return-to-user transition.
        return;
    }
}
