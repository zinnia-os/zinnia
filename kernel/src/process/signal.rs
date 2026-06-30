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

    /// Signals that can never be blocked.
    const UNBLOCKABLE: SignalSet = {
        let mut set = SignalSet::new();
        set.set(Signal::SigKill, true);
        set.set(Signal::SigStop, true);
        set
    };

    /// Remove the unblockable signals (SIGKILL, SIGSTOP) from this set.
    pub fn sanitize_mask(&mut self) {
        *self &= !Self::UNBLOCKABLE;
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

#[derive(Clone, Debug)]
pub struct SigQueue {
    pending: SignalSet,
    info: [SigInfoData; (MAX_SIGNAL + 1) as usize],
}

impl SigQueue {
    pub fn new() -> Self {
        Self {
            pending: SignalSet::new(),
            info: [SigInfoData::default(); (MAX_SIGNAL + 1) as usize],
        }
    }

    pub const fn pending(&self) -> SignalSet {
        self.pending
    }

    pub fn queue(&mut self, sig: Signal, info: SigInfoData) {
        self.pending.set(sig, true);
        self.info[sig as usize] = info;
    }

    pub fn dequeue(&mut self, allowed: SignalSet) -> Option<(Signal, SigInfoData)> {
        let sig = (self.pending & allowed).first_set()?;
        self.pending.set(sig, false);
        Some((sig, self.info[sig as usize]))
    }

    pub fn discard(&mut self, sig: Signal) {
        self.pending.set(sig, false);
    }

    pub fn deliverable(&self, allowed: SignalSet) -> bool {
        !(self.pending & allowed).is_empty()
    }
}

impl Default for SigQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-thread signal state.
#[derive(Clone, Debug, Default)]
pub struct ThreadSignalState {
    /// Signals pending delivery to this thread.
    pub queue: SigQueue,
    /// Current signal mask (signals blocked from delivery).
    pub mask: SignalSet,
    /// The alternate signal stack for this thread.
    pub altstack: AltStack,
    /// Mask to restore once the next signal is delivered.
    pub restore_mask: Option<SignalSet>,
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

/// Discards all pending `sig` (shared + thread queues) when `action` ignores
/// it, as POSIX requires when a disposition becomes SIG_IGN (or ignoring DFL).
pub fn flush_if_ignored(proc: &Arc<Process>, sig: Signal, action: &SigAction) {
    let ignored = action.is_ignore()
        || (action.is_default() && sig.default_action() == DefaultAction::Ignore);
    if !ignored {
        return;
    }
    for thread in proc.threads.lock().iter() {
        thread.signal.lock().queue.discard(sig);
    }
    proc.shared_pending.lock().discard(sig);
}

fn prepare_signal(proc: &Arc<Process>, sig: Signal, blocked: bool) -> bool {
    // SIGCONT/SIGKILL unblock a stopped process even if blocked or ignored.
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

    // SIGCONT discards pending stop signals and vice versa, in every queue of the process.
    let is_stop = STOP_SIGNALS.contains(&sig);
    if sig == Signal::SigCont || is_stop {
        let discard = |queue: &mut SigQueue| {
            if is_stop {
                queue.discard(Signal::SigCont);
            } else {
                for s in STOP_SIGNALS {
                    queue.discard(s);
                }
            }
        };
        for thread in proc.threads.lock().iter() {
            discard(&mut thread.signal.lock().queue);
        }
        discard(&mut proc.shared_pending.lock());
    }

    // Drop ignored signals at send time, unless they are blocked.
    if !sig.is_uncatchable() && !blocked {
        let action = *proc.signal_actions.lock().get_action(sig);
        if action.is_ignore()
            || (action.is_default() && sig.default_action() == DefaultAction::Ignore)
        {
            return false;
        }
    }

    true
}

/// Queue a signal on the given thread and wake it if it is sleeping.
pub fn send_signal_info_to_thread(task: &Arc<Task>, sig: Signal, info: SigInfoData) {
    let proc = task.get_process();
    let blocked = task.signal.lock().mask.is_set(sig);

    if !prepare_signal(&proc, sig, blocked) {
        return;
    }

    task.signal.lock().queue.queue(sig, info);
    proc.signal_event.wake_all();
    Scheduler::wake_task(task.clone());
}

/// Send a process-directed signal with default (kernel-originated) info.
pub fn send_signal_to_process(proc: &Arc<Process>, sig: Signal) -> bool {
    send_signal_info_to_process(proc, sig, SigInfoData::kernel())
}

/// Queue a signal and wake a thread to handle it.
/// Returns false if the process has no threads to deliver to.
pub fn send_signal_info_to_process(proc: &Arc<Process>, sig: Signal, info: SigInfoData) -> bool {
    // Choose which thread to wake; prefer one with the signal unblocked.
    let (target, all_blocked) = {
        let threads = proc.threads.lock();
        let target = threads
            .iter()
            .find(|t| !t.signal.lock().mask.is_set(sig))
            .or_else(|| threads.first())
            .cloned();
        let all_blocked = threads.iter().all(|t| t.signal.lock().mask.is_set(sig));
        (target, all_blocked)
    };

    let Some(target) = target else {
        return false;
    };

    if !prepare_signal(proc, sig, all_blocked) {
        return true;
    }

    proc.shared_pending.lock().queue(sig, info);
    proc.signal_event.wake_all();

    if sig == Signal::SigKill {
        // Wake every thread so the whole process dies promptly.
        let threads = proc.threads.lock().clone();
        for thread in threads {
            Scheduler::wake_task(thread);
        }
    } else {
        Scheduler::wake_task(target);
    }

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
    let proc = task.get_process();
    let blocked = task.signal.lock().mask.is_set(sig);

    {
        let mut actions = proc.signal_actions.lock();
        if actions.get_action(sig).is_ignore() || blocked {
            actions.set_action(sig, SigAction::default());
        }
    }

    let mut state = task.signal.lock();
    state.mask.set(sig, false);
    state.queue.queue(sig, info);
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PendingQueue {
    Thread,
    Process,
}

pub(crate) fn dequeue_signal(
    task: &Task,
    proc: &Process,
    set: SignalSet,
) -> Option<(Signal, SigInfoData, PendingQueue)> {
    let mut state = task.signal.lock();
    if let Some((sig, info)) = state.queue.dequeue(set) {
        return Some((sig, info, PendingQueue::Thread));
    }
    proc.shared_pending
        .lock()
        .dequeue(set)
        .map(|(sig, info)| (sig, info, PendingQueue::Process))
}

pub(crate) fn requeue_signal(
    task: &Task,
    proc: &Process,
    queue: PendingQueue,
    sig: Signal,
    info: SigInfoData,
) {
    match queue {
        PendingQueue::Thread => task.signal.lock().queue.queue(sig, info),
        PendingQueue::Process => proc.shared_pending.lock().queue(sig, info),
    }
    proc.signal_event.wake_all();
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

        // If the process was stopped, park here until SIGCONT/SIGKILL.
        if matches!(*proc.status.lock(), State::Stopped(_)) {
            wait_while_stopped(&proc);
            continue;
        }

        // Dequeue the lowest deliverable signal: thread queue first, then shared.
        let dequeued = {
            let mut state = task.signal.lock();
            let unblocked = !state.mask;
            let dequeued = state
                .queue
                .dequeue(unblocked)
                .or_else(|| proc.shared_pending.lock().dequeue(unblocked));
            if dequeued.is_none() {
                // Restore the sigsuspend/pselect mask once nothing is deliverable.
                if let Some(mask) = state.restore_mask.take() {
                    state.mask = mask;
                }
            }
            dequeued
        };

        let Some((sig, info)) = dequeued else {
            // If a syscall was interrupted by such a signal, restart it transparently.
            if let Some(sc) = syscall.as_ref().filter(|_| restartable) {
                context.restart_syscall(sc);
            }
            return;
        };

        // Apply SA_RESETHAND under the actions lock so it can't fire twice.
        let action = {
            let mut actions = proc.signal_actions.lock();
            let action = *actions.get_action(sig);
            if !action.is_ignore()
                && !action.is_default()
                && action.flags & signal::SA_RESETHAND != 0
            {
                actions.set_action(sig, SigAction::default());
            }
            action
        };

        if action.is_ignore() {
            continue;
        }

        if action.is_default() {
            match sig.default_action() {
                DefaultAction::Ignore | DefaultAction::Continue => continue,
                DefaultAction::Terminate | DefaultAction::CoreDump => {
                    // exit() never returns; drop our Arcs so they aren't stranded.
                    drop(task);
                    drop(proc);
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

        // Stage the mask sigreturn restores, then apply the handler-entry mask.
        let (old_mask, altstack) = {
            let mut state = task.signal.lock();
            let old_mask = state.restore_mask.take().unwrap_or(state.mask);
            let altstack = state.altstack;
            if action.flags & signal::SA_NODEFER == 0 {
                state.mask.set(sig, true);
            }
            state.mask |= action.mask;
            state.mask.sanitize_mask();
            (old_mask, altstack)
        };

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

    wait_while_stopped(proc);
}

fn wait_while_stopped(proc: &Arc<Process>) {
    loop {
        // Register before checking, so a wakeup between check and wait() is not missed.
        let guard = proc.cont_event.guard();
        if !matches!(*proc.status.lock(), State::Stopped(_)) {
            break;
        }
        guard.wait();
    }
}
