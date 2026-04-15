use crate::{
    arch::sched::{Context, jump_to_context},
    memory::{UserPtr, VirtAddr},
    posix::errno::{EResult, Errno},
    process::{
        PROCESS_TABLE, Pid, Process,
        signal::{self, SigAction, Signal, SignalSet},
    },
    sched::Scheduler,
    uapi, wrap_syscall,
};
use alloc::sync::Arc;

#[wrap_syscall]
pub fn sigaction(sig: u32, act_ptr: VirtAddr, oact_ptr: VirtAddr) -> EResult<usize> {
    let sig = Signal::from_raw(sig).ok_or(Errno::EINVAL)?;

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
pub fn kill(pid: isize, sig: usize) -> EResult<usize> {
    let sig_num = sig as u32;

    // Signal 0 is used to check permissions / process existence without sending.
    if sig_num != 0 {
        let _ = Signal::from_raw(sig_num).ok_or(Errno::EINVAL)?;
    }

    match pid {
        _ if pid > 0 => {
            let target = find_process_by_pid(pid as usize).ok_or(Errno::ESRCH)?;

            if sig_num == 0 {
                return Ok(0);
            }

            let sig = Signal::from_raw(sig_num).unwrap();
            if !signal::send_signal_to_process(&target, sig) {
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

            let sig = Signal::from_raw(sig_num).unwrap();
            if signal::send_signal_to_pgrp(pgrp, sig) == 0 {
                return Err(Errno::ESRCH);
            }
            Ok(0)
        }
        -1 => {
            // Send to every process except PID 1 (init) and PID 0 (kernel).
            if sig_num == 0 {
                return Ok(0);
            }

            let sig = Signal::from_raw(sig_num).unwrap();
            let table = PROCESS_TABLE.lock();
            let mut delivered = 0;
            for (&target_pid, proc) in table.iter() {
                if target_pid <= 1 {
                    continue;
                }
                let Some(proc) = proc.upgrade() else { continue };
                if signal::send_signal_to_process(&proc, sig) {
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
            let pgrp = (-pid) as Pid;

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

            let sig = Signal::from_raw(sig_num).unwrap();
            if signal::send_signal_to_pgrp(pgrp, sig) == 0 {
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

/// Find a process by PID using the global process table.
fn find_process_by_pid(pid: usize) -> Option<Arc<Process>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid)?.upgrade()
}
