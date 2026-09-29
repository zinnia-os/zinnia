use crate::{
    arch::sched::{Context, jump_to_context},
    memory::{UserPtr, VirtAddr},
    posix::errno::{EResult, Errno},
    process::{
        PROCESS_TABLE, Process,
        signal::{self, AltStack, SigAction, SigInfoData, Signal, SignalSet},
    },
    sched::Scheduler,
    uapi::{self, pid_t},
    wrap_syscall,
};
use alloc::{sync::Arc, vec::Vec};

fn signal_from_num(sig: usize) -> EResult<Option<Signal>> {
    match u32::try_from(sig).map_err(|_| Errno::EINVAL)? {
        0 => Ok(None),
        num => Signal::try_from(num).map(Some).map_err(|_| Errno::EINVAL),
    }
}

#[wrap_syscall]
pub fn sigaction(sig: u32, act_ptr: VirtAddr, oact_ptr: VirtAddr) -> EResult<usize> {
    let sig = Signal::try_from(sig).map_err(|_| Errno::EINVAL)?;

    if sig.is_uncatchable() {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();

    // Read user memory before locking.
    let new_action = if !act_ptr.is_null() {
        let act: UserPtr<uapi::signal::sigaction> = UserPtr::new(act_ptr);
        Some(SigAction::from_user(&act.read().ok_or(Errno::EFAULT)?))
    } else {
        None
    };

    let old = *proc.signal_actions.lock().get_action(sig);

    if !oact_ptr.is_null() {
        let mut oact: UserPtr<uapi::signal::sigaction> = UserPtr::new(oact_ptr);
        oact.write(old.to_user()).ok_or(Errno::EFAULT)?;
    }

    if let Some(action) = new_action {
        proc.signal_actions.lock().set_action(sig, action);
        signal::flush_if_ignored(&proc, sig, &action);
    }

    Ok(0)
}

#[wrap_syscall]
pub fn sigprocmask(how: usize, set_ptr: VirtAddr, old_ptr: VirtAddr) -> EResult<usize> {
    let task = Scheduler::get_current();

    // Read user memory before locking.
    let new_set = if !set_ptr.is_null() {
        let how = how as u32;
        if !(uapi::signal::SIG_BLOCK..=uapi::signal::SIG_SETMASK).contains(&how) {
            return Err(Errno::EINVAL);
        }
        let set: UserPtr<uapi::signal::sigset_t> = UserPtr::new(set_ptr);
        let mut new_set = SignalSet::from_raw(set.read().ok_or(Errno::EFAULT)?);
        new_set.sanitize_mask();
        Some((how, new_set))
    } else {
        None
    };

    let old_mask = task.signal.lock().mask;

    if !old_ptr.is_null() {
        let mut old: UserPtr<uapi::signal::sigset_t> = UserPtr::new(old_ptr);
        old.write(old_mask.as_raw()).ok_or(Errno::EFAULT)?;
    }

    if let Some((how, new_set)) = new_set {
        let mut sig_state = task.signal.lock();
        match how {
            uapi::signal::SIG_BLOCK => sig_state.mask |= new_set,
            uapi::signal::SIG_UNBLOCK => sig_state.mask &= !new_set,
            uapi::signal::SIG_SETMASK => sig_state.mask = new_set,
            _ => unreachable!(),
        }
        sig_state.mask.sanitize_mask();
    }

    Ok(0)
}

#[wrap_syscall]
pub fn kill(pid: pid_t, sig: usize) -> EResult<pid_t> {
    let sig = signal_from_num(sig)?;
    let sender = Scheduler::get_current().get_process();
    let info = SigInfoData::user(sender.get_pid(), sender.identity.lock().user_id);

    let targets: Vec<Arc<Process>> = if pid > 0 {
        alloc::vec![Process::lookup(pid)?]
    } else {
        let pgrp = match pid {
            0 => Some(*sender.pgrp.lock()),
            -1 => None,
            _ => Some(pid.checked_neg().ok_or(Errno::ESRCH)?),
        };
        // Snapshot first so target selection and delivery never hold the global table lock.
        let processes: Vec<_> = PROCESS_TABLE
            .lock()
            .values()
            .filter_map(alloc::sync::Weak::upgrade)
            .collect();
        processes
            .into_iter()
            .filter(|proc| match pgrp {
                Some(pgrp) => *proc.pgrp.lock() == pgrp,
                // Broadcast excludes the kernel, init, and the caller.
                None => proc.get_pid() > 1 && proc.get_pid() != sender.get_pid(),
            })
            .collect()
    };

    let mut found = false;
    for target in targets {
        found |= match sig {
            Some(sig) => signal::send_signal_info_to_process(&target, sig, info),
            None => true,
        };
    }
    if !found {
        return Err(Errno::ESRCH);
    }
    Ok(0)
}

#[wrap_syscall]
pub fn sigqueue(pid: pid_t, sig: usize, value: usize) -> EResult<usize> {
    let sig = signal_from_num(sig)?;

    let target = Process::lookup(pid)?;

    let Some(sig) = sig else { return Ok(0) };

    let sender = Scheduler::get_current().get_process();
    let info = SigInfoData::queued(sender.get_pid(), sender.identity.lock().user_id, value);

    if !signal::send_signal_info_to_process(&target, sig, info) {
        return Err(Errno::ESRCH);
    }

    Ok(0)
}

pub fn sigreturn(frame: &mut Context) -> ! {
    crate::arch::sched::restore_signal_frame(frame);

    crate::process::signal::deliver_pending_signals(frame, None);

    unsafe { jump_to_context(frame) };
    unreachable!();
}

#[wrap_syscall]
pub fn sigpending(set_ptr: VirtAddr) -> EResult<usize> {
    let task = Scheduler::get_current();
    let proc = task.get_process();
    let (thread_pending, mask) = {
        let state = task.signal.lock();
        (state.queue.pending(), state.mask)
    };
    let shared_pending = proc.shared_pending.lock().pending();
    let pending = ((thread_pending | shared_pending) & mask).as_raw();
    let mut ptr: UserPtr<uapi::signal::sigset_t> = UserPtr::new(set_ptr);
    ptr.write(pending).ok_or(Errno::EFAULT)?;
    Ok(0)
}

#[wrap_syscall]
pub fn sigtimedwait(
    set_ptr: VirtAddr,
    info_ptr: VirtAddr,
    timeout_ptr: VirtAddr,
) -> EResult<usize> {
    let task = Scheduler::get_current();
    let proc = task.get_process();

    let set: UserPtr<uapi::signal::sigset_t> = UserPtr::new(set_ptr);
    let mut wait_set = SignalSet::from_raw(set.read().ok_or(Errno::EFAULT)?);
    // SIGKILL and SIGSTOP cannot be waited for.
    wait_set.sanitize_mask();

    let deadline = super::system::read_timeout_deadline(timeout_ptr)?;
    let timeout_guard = deadline.map(crate::clock::timeout_at);

    loop {
        let guard = proc.signal_event.guard();

        if let Some((sig, info, queue)) = signal::dequeue_signal(&task, &proc, wait_set) {
            if !info_ptr.is_null() {
                let wrote = UserPtr::<uapi::signal::siginfo_t>::new(info_ptr)
                    .write(info.to_user(sig))
                    .is_some();
                if !wrote {
                    signal::requeue_signal(&task, &proc, queue, sig, info);
                    return Err(Errno::EFAULT);
                }
            }
            return Ok(sig as usize);
        }

        // A deliverable signal outside the waited set interrupts the wait.
        if task.has_pending_signals() {
            return Err(Errno::EINTR);
        }

        if timeout_guard.as_ref().is_some_and(|g| g.expired()) {
            return Err(Errno::EAGAIN);
        }

        guard.wait();
    }
}

#[wrap_syscall]
pub fn sigsuspend(set_ptr: VirtAddr) -> EResult<usize> {
    let task = Scheduler::get_current();
    let proc = task.get_process();

    let set: UserPtr<uapi::signal::sigset_t> = UserPtr::new(set_ptr);
    let mut new_mask = SignalSet::from_raw(set.read().ok_or(Errno::EFAULT)?);
    new_mask.sanitize_mask();

    {
        let mut state = task.signal.lock();
        let old = state.mask;
        state.mask = new_mask;
        state.restore_mask = Some(old);
    }

    // Block until a signal becomes deliverable under the temporary mask.
    loop {
        let guard = proc.signal_event.guard();
        if task.has_pending_signals() {
            break;
        }
        guard.wait();
    }

    Err(Errno::EINTR)
}

pub fn sigaltstack(frame: &mut Context) -> EResult<usize> {
    let ss_ptr = VirtAddr::new(frame.arg0());
    let oss_ptr = VirtAddr::new(frame.arg1());
    let user_sp = frame.sp();

    let task = Scheduler::get_current();
    let current = task.signal.lock().altstack;
    let on_stack = current.contains(user_sp);

    if !oss_ptr.is_null() {
        let oss = uapi::signal::stack_t {
            ss_sp: UserPtr::new(VirtAddr::new(current.sp)),
            ss_size: current.size,
            ss_flags: current.ss_flags(user_sp),
        };
        UserPtr::<uapi::signal::stack_t>::new(oss_ptr)
            .write(oss)
            .ok_or(Errno::EFAULT)?;
    }

    if !ss_ptr.is_null() {
        // The alt stack cannot be changed while executing on it.
        if on_stack {
            return Err(Errno::EPERM);
        }

        let ss = UserPtr::<uapi::signal::stack_t>::new(ss_ptr)
            .read()
            .ok_or(Errno::EFAULT)?;

        if ss.ss_flags != 0
            && ss.ss_flags != uapi::signal::SS_ONSTACK as i32
            && ss.ss_flags != uapi::signal::SS_DISABLE as i32
        {
            return Err(Errno::EINVAL);
        }

        let new = if ss.ss_flags == uapi::signal::SS_DISABLE as i32 {
            AltStack::default()
        } else {
            if ss.ss_size < uapi::signal::MINSIGSTKSZ as usize {
                return Err(Errno::ENOMEM);
            }
            ss.ss_sp
                .addr()
                .value()
                .checked_add(ss.ss_size)
                .ok_or(Errno::EINVAL)?;
            AltStack {
                sp: ss.ss_sp.addr().value(),
                size: ss.ss_size,
                disabled: false,
            }
        };

        task.signal.lock().altstack = new;
    }

    Ok(0)
}
