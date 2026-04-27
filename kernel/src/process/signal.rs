use alloc::sync::Arc;

use crate::{
    arch::sched::Context,
    process::{Process, ProcessState, task::Task},
    sched::Scheduler,
    uapi::{
        pid_t,
        signal::{self, MAX_SIGNAL, SIG_DFL, SIG_IGN, sigaction},
    },
};
use core::{ops, sync::atomic::Ordering};

/// POSIX signals represented as an enum.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    SigAbrt = signal::SIGABRT,
    SigAlrm = signal::SIGALRM,
    SigBus = signal::SIGBUS,
    SigChld = signal::SIGCHLD,
    SigCont = signal::SIGCONT,
    SigFpe = signal::SIGFPE,
    SigHup = signal::SIGHUP,
    SigIll = signal::SIGILL,
    SigInt = signal::SIGINT,
    SigKill = signal::SIGKILL,
    SigPipe = signal::SIGPIPE,
    SigQuit = signal::SIGQUIT,
    SigSegv = signal::SIGSEGV,
    SigStop = signal::SIGSTOP,
    SigTerm = signal::SIGTERM,
    SigTstp = signal::SIGTSTP,
    SigTtin = signal::SIGTTIN,
    SigTtou = signal::SIGTTOU,
    SigUsr1 = signal::SIGUSR1,
    SigUsr2 = signal::SIGUSR2,
    SigWinch = signal::SIGWINCH,
    SigSys = signal::SIGSYS,
    SigTrap = signal::SIGTRAP,
    SigUrg = signal::SIGURG,
    SigVtAlarm = signal::SIGVTALRM,
    SigXCpu = signal::SIGXCPU,
    SigXFsz = signal::SIGXFSZ,
    SigIo = signal::SIGIO,
    SigPoll = signal::SIGPOLL,
    SigProf = signal::SIGPROF,
    SigPwr = signal::SIGPWR,
    SigIot = signal::SIGIOT,
    SigCancel = signal::SIGCANCEL,
}

impl Signal {
    /// Try to convert a raw signal number to a [`Signal`].
    pub const fn from_raw(num: u32) -> Option<Self> {
        match num {
            signal::SIGABRT => Some(Self::SigAbrt),
            signal::SIGALRM => Some(Self::SigAlrm),
            signal::SIGBUS => Some(Self::SigBus),
            signal::SIGCHLD => Some(Self::SigChld),
            signal::SIGCONT => Some(Self::SigCont),
            signal::SIGFPE => Some(Self::SigFpe),
            signal::SIGHUP => Some(Self::SigHup),
            signal::SIGILL => Some(Self::SigIll),
            signal::SIGINT => Some(Self::SigInt),
            signal::SIGKILL => Some(Self::SigKill),
            signal::SIGPIPE => Some(Self::SigPipe),
            signal::SIGQUIT => Some(Self::SigQuit),
            signal::SIGSEGV => Some(Self::SigSegv),
            signal::SIGSTOP => Some(Self::SigStop),
            signal::SIGTERM => Some(Self::SigTerm),
            signal::SIGTSTP => Some(Self::SigTstp),
            signal::SIGTTIN => Some(Self::SigTtin),
            signal::SIGTTOU => Some(Self::SigTtou),
            signal::SIGUSR1 => Some(Self::SigUsr1),
            signal::SIGUSR2 => Some(Self::SigUsr2),
            signal::SIGWINCH => Some(Self::SigWinch),
            signal::SIGSYS => Some(Self::SigSys),
            signal::SIGTRAP => Some(Self::SigTrap),
            signal::SIGURG => Some(Self::SigUrg),
            signal::SIGVTALRM => Some(Self::SigVtAlarm),
            signal::SIGXCPU => Some(Self::SigXCpu),
            signal::SIGXFSZ => Some(Self::SigXFsz),
            signal::SIGIO => Some(Self::SigIo),
            signal::SIGPOLL => Some(Self::SigPoll),
            signal::SIGPROF => Some(Self::SigProf),
            signal::SIGPWR => Some(Self::SigPwr),
            signal::SIGIOT => Some(Self::SigIot),
            signal::SIGCANCEL => Some(Self::SigCancel),
            _ => None,
        }
    }

    pub const fn as_raw(self) -> u32 {
        self as u32
    }

    /// Returns the default action for this signal.
    pub const fn default_action(self) -> DefaultAction {
        match self {
            Signal::SigHup
            | Signal::SigInt
            | Signal::SigPipe
            | Signal::SigAlrm
            | Signal::SigTerm
            | Signal::SigUsr1
            | Signal::SigUsr2
            | Signal::SigProf
            | Signal::SigVtAlarm
            | Signal::SigIo
            | Signal::SigPoll
            | Signal::SigPwr => DefaultAction::Terminate,
            Signal::SigQuit
            | Signal::SigAbrt
            | Signal::SigBus
            | Signal::SigFpe
            | Signal::SigIll
            | Signal::SigSegv
            | Signal::SigSys
            | Signal::SigTrap
            | Signal::SigXCpu
            | Signal::SigXFsz
            | Signal::SigIot => DefaultAction::CoreDump,
            Signal::SigStop | Signal::SigTstp | Signal::SigTtin | Signal::SigTtou => {
                DefaultAction::Stop
            }
            Signal::SigCont => DefaultAction::Continue,
            Signal::SigChld | Signal::SigUrg | Signal::SigWinch => DefaultAction::Ignore,
            Signal::SigKill | Signal::SigCancel => DefaultAction::Terminate,
        }
    }

    /// Returns true if this signal cannot be caught or ignored.
    pub fn is_uncatchable(self) -> bool {
        matches!(self, Signal::SigKill | Signal::SigStop)
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
const UNBLOCKABLE: SignalSet = {
    let mut set = SignalSet::new();
    set.set(Signal::SigKill, true);
    set.set(Signal::SigStop, true);
    set
};

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

    pub const fn set(&mut self, sig: Signal, state: bool) {
        let bit = 1u64 << sig.as_raw();
        if state {
            self.inner |= bit;
        } else {
            self.inner &= !bit;
        }
    }

    pub const fn is_set(&self, sig: Signal) -> bool {
        self.inner & (1u64 << sig.as_raw()) != 0
    }

    pub const fn is_empty(&self) -> bool {
        self.inner == 0
    }

    /// Returns the lowest signal number that is set, or None.
    pub const fn first_set(&self) -> Option<Signal> {
        if self.inner == 0 {
            return None;
        }
        let bit = self.inner.trailing_zeros();
        Signal::from_raw(bit)
    }

    /// Remove the unblockable signals (SIGKILL, SIGSTOP) from this set.
    pub fn sanitize_mask(&mut self) {
        *self &= !UNBLOCKABLE;
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
pub fn send_signal_to_thread(task: &Arc<Task>, sig: Signal) {
    let proc = task.get_process();

    if !sig.is_uncatchable() {
        let action = *proc.signal_actions.lock().get_action(sig);
        if action.is_ignore()
            || (action.is_default() && sig.default_action() == DefaultAction::Ignore)
        {
            return;
        }
    }

    // SIGCONT (and SIGKILL) must unblock a stopped process so it can run again.
    if sig == Signal::SigCont || sig == Signal::SigKill {
        let was_stopped = {
            let mut state = proc.status.lock();
            if matches!(*state, ProcessState::Stopped(_)) {
                *state = ProcessState::Running;
                proc.continue_unwaited.store(true, Ordering::Release);
                true
            } else {
                false
            }
        };
        if was_stopped {
            proc.cont_event.wake_all();
            notify_parent_of_child_state_change(&proc, false);
        }
    }

    {
        let mut state = task.signal.lock();
        state.pending.set(sig, true);
    }
    proc.signalfd_event.wake_all();
    Scheduler::wake_task(task.clone());
}

pub fn send_signal_to_process(proc: &Arc<Process>, sig: Signal) -> bool {
    let thread = {
        let threads = proc.threads.lock();
        threads.first().cloned()
    };

    let Some(thread) = thread else {
        return false;
    };

    send_signal_to_thread(&thread, sig);
    true
}

pub fn notify_parent_of_child_state_change(proc: &Arc<Process>, stopped: bool) {
    let Some(parent) = proc.get_parent() else {
        return;
    };

    parent.child_event.wake_all();

    if stopped {
        let action = *parent.signal_actions.lock().get_action(Signal::SigChld);
        if action.flags & signal::SA_NOCLDSTOP != 0 {
            return;
        }
    }

    send_signal_to_process(&parent, Signal::SigChld);
}

pub fn force_signal_to_thread(task: &Task, sig: Signal) {
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
pub fn send_signal_to_pgrp(pgrp: pid_t, sig: Signal) -> usize {
    let table = crate::process::PROCESS_TABLE.lock();
    let mut delivered = 0;

    for proc in table.values() {
        let Some(proc) = proc.upgrade() else { continue }; // TODO: Should entries be removed?
        if *proc.pgrp.lock() == pgrp && send_signal_to_process(&proc, sig) {
            delivered += 1;
        }
    }

    delivered
}

/// Deliver pending signals to the current thread. Called before returning to
/// userspace. May modify the context to redirect execution to a user handler,
/// terminate the process, or block until SIGCONT (for stop signals).
///
/// At most one user handler is set up per call; ignored and default-ignore
/// signals are consumed in the same pass so a later important signal behind
/// them in the bitmap is not delayed.
pub fn deliver_pending_signals(context: &mut Context) {
    loop {
        let task = Scheduler::get_current();
        let proc = task.get_process();

        let sig = {
            let state = task.signal.lock();
            match (state.pending & !state.mask).first_set() {
                Some(s) => s,
                None => return,
            }
        };

        task.signal.lock().pending.set(sig, false);

        let action = *proc.signal_actions.lock().get_action(sig);

        if action.is_ignore() {
            continue;
        }

        if action.is_default() {
            match sig.default_action() {
                DefaultAction::Ignore | DefaultAction::Continue => continue,
                DefaultAction::Terminate | DefaultAction::CoreDump => {
                    Process::exit(0x7f + sig.as_raw() as u8);
                }
                DefaultAction::Stop => {
                    enter_stopped_state(&proc, sig);
                    continue;
                }
            }
        }

        let old_mask = task.signal.lock().mask;

        {
            let mut state = task.signal.lock();
            if action.flags & signal::SA_NODEFER == 0 {
                state.mask.set(sig, true);
            }
            state.mask |= action.mask;
            state.mask.sanitize_mask();
        }

        if action.flags & signal::SA_RESETHAND != 0 {
            proc.signal_actions
                .lock()
                .set_action(sig, SigAction::default());
        }

        crate::arch::sched::setup_signal_frame(
            context,
            action.handler,
            sig.as_raw(),
            old_mask,
            action.restorer,
        );

        return;
    }
}

/// Transition the process into the Stopped state and block until SIGCONT.
/// Called from  [`deliver_pending_signals`] when a stop signal's default action fires.
fn enter_stopped_state(proc: &Arc<Process>, sig: Signal) {
    *proc.status.lock() = ProcessState::Stopped(sig);
    proc.stop_unwaited.store(true, Ordering::Release);
    notify_parent_of_child_state_change(proc, true);

    // Park on cont_event until SIGCONT (or SIGKILL) flips us back to Running.
    loop {
        let guard = proc.cont_event.guard();
        if !matches!(*proc.status.lock(), ProcessState::Stopped(_)) {
            break;
        }
        guard.wait();
    }
}
