use crate::{
    arch::sched::{Context, SyscallRestart},
    memory::{UserPtr, VirtAddr},
    posix::errno::Errno,
    process::{Process, State, task::Task},
    sched::Scheduler,
    uapi::{
        self, pid_t,
        signal::{self, MAX_SIGNAL, SIG_DFL, SIG_IGN, sigaction, siginfo_t, sigval},
        uid_t,
    },
};
use alloc::sync::Arc;
use core::{ops, sync::atomic::Ordering};
use num_enum::TryFromPrimitive;

/// POSIX signals represented as an enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
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

    const fn bit(sig: Signal) -> u64 {
        1u64 << (sig as u32 - 1)
    }

    pub const fn set(&mut self, sig: Signal, state: bool) {
        let bit = Self::bit(sig);
        if state {
            self.inner |= bit;
        } else {
            self.inner &= !bit;
        }
    }

    pub const fn is_set(&self, sig: Signal) -> bool {
        self.inner & Self::bit(sig) != 0
    }

    pub const fn is_empty(&self) -> bool {
        self.inner == 0
    }

    /// Returns the lowest signal number that is set, or None.
    pub fn first_set(&self) -> Option<Signal> {
        if self.inner == 0 {
            return None;
        }
        let bit = self.inner.trailing_zeros();
        Signal::try_from(bit + 1).ok()
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
        &self.actions[sig as usize]
    }

    pub fn set_action(&mut self, sig: Signal, action: SigAction) {
        self.actions[sig as usize] = action;
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

/// Per-signal information delivered to [`uapi::signal::SA_SIGINFO`] handlers.
#[derive(Clone, Copy, Debug, Default)]
pub struct SigInfoData {
    pub code: i32,
    pub errno: i32,
    pub pid: pid_t,
    pub uid: uid_t,
    pub addr: usize,
    pub status: i32,
}

impl SigInfoData {
    /// Info for a signal raised by the kernel.
    pub fn kernel() -> Self {
        Self {
            code: signal::SI_KERNEL as i32,
            ..Default::default()
        }
    }

    /// Info for a signal sent by a process via kill/tkill.
    pub fn user(sender: pid_t, uid: uid_t) -> Self {
        Self {
            code: signal::SI_USER as i32,
            pid: sender,
            uid,
            ..Default::default()
        }
    }

    /// Convert to the userspace [`siginfo_t`] for a given signal number.
    pub fn to_user(self, sig: Signal) -> siginfo_t {
        siginfo_t {
            si_signo: sig as i32,
            si_code: self.code,
            si_errno: self.errno,
            si_pid: self.pid,
            si_uid: self.uid,
            si_addr: UserPtr::new(VirtAddr::new(self.addr)),
            si_status: self.status,
            si_value: sigval { sival_int: 0 },
        }
    }
}

/// The alternate signal stack registered via `sigaltstack`.
#[derive(Clone, Copy, Debug, Default)]
pub struct AltStack {
    pub sp: usize,
    pub size: usize,
    pub flags: i32,
}

impl AltStack {
    /// Whether an alternate stack is registered and usable.
    pub fn is_enabled(&self) -> bool {
        self.flags & signal::SS_DISABLE as i32 == 0 && self.size != 0
    }

    /// Whether the given stack pointer currently points inside this stack.
    pub fn contains(&self, sp: usize) -> bool {
        self.is_enabled() && sp > self.sp && sp <= self.sp + self.size
    }
}

pub struct SignalDelivery {
    pub handler: usize,
    pub signal: u32,
    pub info: siginfo_t,
    pub old_mask: SignalSet,
    pub flags: u32,
    pub restorer: usize,
    pub altstack: AltStack,
}

/// Per-thread signal state.
#[derive(Clone, Debug)]
pub struct ThreadSignalState {
    /// Signals pending delivery to this thread.
    pub pending: SignalSet,
    /// Current signal mask (signals blocked from delivery).
    pub mask: SignalSet,
    /// Per-signal info for the currently pending signals.
    pub pending_info: [SigInfoData; (MAX_SIGNAL + 1) as usize],
    /// The alternate signal stack for this thread.
    pub altstack: AltStack,
    /// Mask to restore once the next signal is delivered.
    pub restore_mask: Option<SignalSet>,
}

impl Default for ThreadSignalState {
    fn default() -> Self {
        Self {
            pending: SignalSet::new(),
            mask: SignalSet::new(),
            pending_info: [SigInfoData::default(); (MAX_SIGNAL + 1) as usize],
            altstack: AltStack::default(),
            restore_mask: None,
        }
    }
}

/// Signals whose default action stops the process.
const STOP_SIGNALS: [Signal; 4] = [
    Signal::SigStop,
    Signal::SigTstp,
    Signal::SigTtin,
    Signal::SigTtou,
];

pub fn send_signal_to_thread(task: &Arc<Task>, sig: Signal) {
    send_signal_info_to_thread(task, sig, SigInfoData::kernel());
}

/// Queue a signal on the given thread and wake it if it is sleeping.
pub fn send_signal_info_to_thread(task: &Arc<Task>, sig: Signal, info: SigInfoData) {
    let proc = task.get_process();

    // SIGCONT (and SIGKILL) must unblock a stopped process so it can run again.
    if sig == Signal::SigCont || sig == Signal::SigKill {
        let was_stopped = {
            let mut state = proc.status.lock();
            if matches!(*state, State::Stopped(_)) {
                *state = State::Running;
                proc.continue_unwaited.store(true, Ordering::Release);
                true
            } else {
                false
            }
        };
        if was_stopped {
            proc.cont_event.wake_all();
            if sig == Signal::SigCont {
                notify_parent_of_child_state_change(
                    &proc,
                    signal::CLD_CONTINUED as i32,
                    Signal::SigCont as i32,
                );
            }
        }
    }

    if !sig.is_uncatchable() {
        let action = *proc.signal_actions.lock().get_action(sig);
        if action.is_ignore()
            || (action.is_default() && sig.default_action() == DefaultAction::Ignore)
        {
            return;
        }
    }

    {
        let mut state = task.signal.lock();
        // SIGCONT discards pending stop signals and vice versa.
        if sig == Signal::SigCont {
            for s in STOP_SIGNALS {
                state.pending.set(s, false);
            }
        } else if STOP_SIGNALS.contains(&sig) {
            state.pending.set(Signal::SigCont, false);
        }
        state.pending.set(sig, true);
        state.pending_info[sig as usize] = info;
    }
    proc.signalfd_event.wake_all();
    Scheduler::wake_task(task.clone());
}

/// Send a process-directed signal with default (kernel-originated) info.
pub fn send_signal_to_process(proc: &Arc<Process>, sig: Signal) -> bool {
    send_signal_info_to_process(proc, sig, SigInfoData::kernel())
}

/// Deliver a process-directed signal, choosing a thread that has it unblocked.
pub fn send_signal_info_to_process(proc: &Arc<Process>, sig: Signal, info: SigInfoData) -> bool {
    let target = {
        let threads = proc.threads.lock();
        threads
            .iter()
            .find(|t| !t.signal.lock().mask.is_set(sig))
            .or_else(|| threads.first())
            .cloned()
    };

    let Some(target) = target else {
        return false;
    };

    send_signal_info_to_thread(&target, sig, info);
    true
}

pub fn notify_parent_of_child_state_change(proc: &Arc<Process>, code: i32, status: i32) {
    let Some(parent) = proc.get_parent() else {
        return;
    };

    parent.child_event.wake_all();

    if code == signal::CLD_STOPPED as i32 {
        let action = *parent.signal_actions.lock().get_action(Signal::SigChld);
        if action.flags & signal::SA_NOCLDSTOP != 0 {
            return;
        }
    }

    let info = SigInfoData {
        code,
        pid: proc.get_pid(),
        uid: proc.identity.lock().user_id,
        status,
        ..Default::default()
    };
    send_signal_info_to_process(&parent, Signal::SigChld, info);
}

pub fn force_signal_to_thread(task: &Task, sig: Signal, info: SigInfoData) {
    // Unmask the signal so it's deliverable.
    {
        let mut state = task.signal.lock();
        state.mask.set(sig, false);
        state.pending.set(sig, true);
        state.pending_info[sig as usize] = info;
    }

    // Reset the handler to SIG_DFL so the default action (terminate) is taken.
    let proc = task.get_process();
    proc.signal_actions
        .lock()
        .set_action(sig, SigAction::default());
}

/// Send a signal to every process in the given process group.
pub fn send_signal_to_pgrp(pgrp: pid_t, sig: Signal) -> usize {
    send_signal_info_to_pgrp(pgrp, sig, SigInfoData::kernel())
}

/// Send a signal with the given info to every process in a process group.
pub fn send_signal_info_to_pgrp(pgrp: pid_t, sig: Signal, info: SigInfoData) -> usize {
    let table = crate::process::PROCESS_TABLE.lock();
    let mut delivered = 0;

    for proc in table.values() {
        let Some(proc) = proc.upgrade() else { continue }; // TODO: Should entries be removed?
        if *proc.pgrp.lock() == pgrp && send_signal_info_to_process(&proc, sig, info) {
            delivered += 1;
        }
    }

    delivered
}

pub fn deliver_pending_signals(context: &mut Context, syscall: Option<SyscallRestart>) {
    let restartable =
        syscall.is_some() && context.syscall_error() == uapi::errno::ERESTART as usize;

    loop {
        let task = Scheduler::get_current();
        let proc = task.get_process();

        let sig = {
            let state = task.signal.lock();
            (state.pending & !state.mask).first_set()
        };

        let Some(sig) = sig else {
            // If a syscall was interrupted by such a signal, restart it transparently.
            if let Some(sc) = syscall.as_ref().filter(|_| restartable) {
                context.restart_syscall(sc);
            }

            let mut state = task.signal.lock();
            if let Some(mask) = state.restore_mask.take() {
                state.mask = mask;
            }
            return;
        };

        let info = {
            let mut state = task.signal.lock();
            state.pending.set(sig, false);
            state.pending_info[sig as usize]
        };

        let action = *proc.signal_actions.lock().get_action(sig);

        if action.is_ignore() {
            continue;
        }

        if action.is_default() {
            match sig.default_action() {
                DefaultAction::Ignore | DefaultAction::Continue => continue,
                DefaultAction::Terminate | DefaultAction::CoreDump => {
                    Process::exit(State::Signaled(sig));
                }
                DefaultAction::Stop => {
                    enter_stopped_state(&proc, sig);
                    continue;
                }
            }
        }

        if restartable {
            if let Some(sc) = syscall.as_ref() {
                if action.flags & signal::SA_RESTART != 0 {
                    context.restart_syscall(sc);
                } else {
                    context.set_return(0, Errno::EINTR as usize);
                }
            }
        }

        // The mask restored on sigreturn, or otherwise the current mask.
        let (old_mask, altstack) = {
            let mut state = task.signal.lock();
            let old_mask = state.restore_mask.take().unwrap_or(state.mask);
            (old_mask, state.altstack)
        };

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

        let delivery = SignalDelivery {
            handler: action.handler,
            signal: sig as u32,
            info: info.to_user(sig),
            old_mask,
            flags: action.flags,
            restorer: action.restorer,
            altstack,
        };
        crate::arch::sched::setup_signal_frame(context, &delivery);

        return;
    }
}

/// Transition the process into the Stopped state and block until SIGCONT.
/// Called from  [`deliver_pending_signals`] when a stop signal's default action fires.
fn enter_stopped_state(proc: &Arc<Process>, sig: Signal) {
    *proc.status.lock() = State::Stopped(sig);
    proc.stop_unwaited.store(true, Ordering::Release);
    notify_parent_of_child_state_change(proc, signal::CLD_STOPPED as i32, sig as i32);

    // Park on cont_event until SIGCONT (or SIGKILL) flips us out of Stopped.
    loop {
        let guard = proc.cont_event.guard();
        if !matches!(*proc.status.lock(), State::Stopped(_)) {
            break;
        }
        guard.wait();
    }
}
