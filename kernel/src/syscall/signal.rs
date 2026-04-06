use crate::{
    arch::sched::Context,
    memory::{UserPtr, VirtAddr},
    posix::errno::{EResult, Errno},
    process::{
        Process,
        signal::{self, SigAction, Signal, SignalSet},
    },
    sched::Scheduler,
    uapi,
};
use alloc::sync::Arc;

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
            // Send to the first thread of the target process.
            let threads = target.threads.lock();
            if let Some(thread) = threads.first() {
                signal::send_signal_to_thread(thread, sig);
            }

            Ok(0)
        }
        // pid == 0: send to every process in the caller's process group.
        // pid == -1: send to every process (except init).
        // pid < -1: send to every process in process group |pid|.
        _ => Err(Errno::ESRCH),
    }
}

pub fn sigreturn(frame: &mut Context) -> EResult<usize> {
    crate::arch::sched::restore_signal_frame(frame)?;
    // The return value is embedded in the restored context, so this value is ignored.
    Ok(0)
}

/// Walk the process tree from the kernel process to find a process by PID.
fn find_process_by_pid(pid: usize) -> Option<Arc<Process>> {
    let kernel = Process::get_kernel();

    // Check the kernel process itself.
    if kernel.get_pid() == pid {
        return Some(kernel.clone());
    }

    find_in_children(kernel, pid)
}

fn find_in_children(proc: &Arc<Process>, pid: usize) -> Option<Arc<Process>> {
    let children = proc.children.lock();
    for child in children.iter() {
        if child.get_pid() == pid {
            return Some(child.clone());
        }
        if let Some(found) = find_in_children(child, pid) {
            return Some(found);
        }
    }
    None
}
