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

#[wrap_syscall]
pub fn sigaction(sig: u32, act_ptr: VirtAddr, oact_ptr: VirtAddr) -> EResult<usize> {
    let sig = Signal::try_from(sig).map_err(|_| Errno::EINVAL)?;

    if sig.is_uncatchable() {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let mut actions = proc.signal_actions.lock();

    // Return old action if requested.
    if !oact_ptr.is_null() {
        let old = actions.get_action(sig).to_user();
        let mut oact: UserPtr<uapi::signal::sigaction> = UserPtr::new(oact_ptr);
        oact.write(old).ok_or(Errno::EFAULT)?;
    }

    // Install new action if provided.
    if !act_ptr.is_null() {
        let act: UserPtr<uapi::signal::sigaction> = UserPtr::new(act_ptr);
        let user_act = act.read().ok_or(Errno::EFAULT)?;
        let action = SigAction::from_user(&user_act);
        actions.set_action(sig, action);
    }

    Ok(0)
}

#[wrap_syscall]
pub fn sigprocmask(how: usize, set_ptr: VirtAddr, old_ptr: VirtAddr) -> EResult<usize> {
    let task = Scheduler::get_current();
    let mut sig_state = task.signal.lock();

    // Return old mask if requested.
    if !old_ptr.is_null() {
        let mut old: UserPtr<uapi::signal::sigset_t> = UserPtr::new(old_ptr);
        old.write(sig_state.mask.as_raw()).ok_or(Errno::EFAULT)?;
    }

    // Modify mask if set is provided.
    if !set_ptr.is_null() {
        let set: UserPtr<uapi::signal::sigset_t> = UserPtr::new(set_ptr);
        let raw_set = set.read().ok_or(Errno::EFAULT)?;
        let mut new_set = SignalSet::from_raw(raw_set);
        new_set.sanitize_mask();

        match how as u32 {
            uapi::signal::SIG_BLOCK => {
                sig_state.mask |= new_set;
            }
            uapi::signal::SIG_UNBLOCK => {
                sig_state.mask = sig_state.mask & !new_set;
            }
            uapi::signal::SIG_SETMASK => {
                sig_state.mask = new_set;
            }
            _ => return Err(Errno::EINVAL),
        }

        // Ensure SIGKILL and SIGSTOP are never blocked.
        sig_state.mask.sanitize_mask();
    }

    Ok(0)
}

#[wrap_syscall]
pub fn kill(pid: pid_t, sig: usize) -> EResult<pid_t> {
    let sig_num = sig as u32;

    // Signal 0 is used to check permissions / process existence without sending.
    if sig_num != 0 {
        let _ = Signal::try_from(sig_num).map_err(|_| Errno::EINVAL)?;
    }

    let sender = Scheduler::get_current().get_process();
    let info = SigInfoData::user(sender.get_pid(), sender.identity.lock().user_id);

    match pid {
        _ if pid > 0 => {
            let target = find_process_by_pid(pid).ok_or(Errno::ESRCH)?;

            if sig_num == 0 {
                return Ok(0);
            }

            let sig = Signal::try_from(sig_num).unwrap();
            if !signal::send_signal_info_to_process(&target, sig, info) {
                return Err(Errno::ESRCH);
            }

            Ok(0)
        }
        0 => {
            // Send to every process in the caller's process group.
            let pgrp = *Scheduler::get_current().get_process().pgrp.lock();

            if sig_num == 0 {
                return Ok(0);
            }

            let sig = Signal::try_from(sig_num).unwrap();
            if signal::send_signal_info_to_pgrp(pgrp, sig, info) == 0 {
                return Err(Errno::ESRCH);
            }
            Ok(0)
        }
        -1 => {
            // Send to every process except PID 1 (init) and PID 0 (kernel).
            if sig_num == 0 {
                return Ok(0);
            }

            let sig = Signal::try_from(sig_num).unwrap();
            let targets = {
                let table = PROCESS_TABLE.lock();
                table
                    .iter()
                    .filter_map(|(&target_pid, proc)| {
                        if target_pid <= 1 {
                            return None;
                        }
                        proc.upgrade()
                    })
                    .collect::<Vec<_>>()
            };

            let mut delivered = 0;
            for proc in targets {
                if signal::send_signal_info_to_process(&proc, sig, info) {
                    delivered += 1;
                }
            }

            if delivered == 0 {
                return Err(Errno::ESRCH);
            }

            Ok(0)
        }
        _ => {
            // pid < -1: send to every process in process group |pid|.
            let pgrp = -pid;

            if sig_num == 0 {
                // Check if any process exists in this group.
                let table = PROCESS_TABLE.lock();
                let exists = table.values().any(|p| {
                    let Some(p) = p.upgrade() else { return false };
                    *p.pgrp.lock() == pgrp
                });
                if !exists {
                    return Err(Errno::ESRCH);
                }
                return Ok(0);
            }

            let sig = Signal::try_from(sig_num).unwrap();
            if signal::send_signal_info_to_pgrp(pgrp, sig, info) == 0 {
                return Err(Errno::ESRCH);
            }
            Ok(0)
        }
    }
}

pub fn sigreturn(frame: &mut Context) -> ! {
    crate::arch::sched::restore_signal_frame(frame);

    unsafe { jump_to_context(frame) };
    unreachable!();
}

#[wrap_syscall]
pub fn sigpending(set_ptr: VirtAddr) -> EResult<usize> {
    let task = Scheduler::get_current();
    let pending = task.signal.lock().pending.as_raw();
    let mut ptr: UserPtr<uapi::signal::sigset_t> = UserPtr::new(set_ptr);
    ptr.write(pending).ok_or(Errno::EFAULT)?;
    Ok(0)
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
        let guard = proc.signalfd_event.guard();
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
        let flags = if on_stack {
            uapi::signal::SS_ONSTACK as i32
        } else if !current.is_enabled() {
            uapi::signal::SS_DISABLE as i32
        } else {
            0
        };
        let oss = uapi::signal::stack_t {
            ss_sp: UserPtr::new(VirtAddr::new(current.sp)),
            ss_size: current.size,
            ss_flags: flags,
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

        let new = if ss.ss_flags & uapi::signal::SS_DISABLE as i32 != 0 {
            AltStack {
                sp: 0,
                size: 0,
                flags: uapi::signal::SS_DISABLE as i32,
            }
        } else {
            if ss.ss_flags & !(uapi::signal::SS_ONSTACK as i32) != 0 {
                return Err(Errno::EINVAL);
            }
            if ss.ss_size < uapi::signal::MINSIGSTKSZ as usize {
                return Err(Errno::ENOMEM);
            }
            AltStack {
                sp: ss.ss_sp.addr().value(),
                size: ss.ss_size,
                flags: 0,
            }
        };

        task.signal.lock().altstack = new;
    }

    Ok(0)
}

/// Find a process by PID using the global process table.
fn find_process_by_pid(pid: pid_t) -> Option<Arc<Process>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid)?.upgrade()
}
