use core::sync::atomic::Ordering;

use crate::{
    arch::{self, sched::Context},
    memory::{UserCStr, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::{
        PROCESS_TABLE, Process, ProcessState,
        signal::{self, Signal},
        to_user,
    },
    sched::Scheduler,
    uapi::{self, limits::PATH_MAX, uid_t},
    vfs::{File, file::OpenFlags, inode::Mode},
    wrap_syscall,
};
use alloc::{string::String, sync::Arc, vec::Vec};

#[wrap_syscall]
pub fn gettid() -> usize {
    Scheduler::get_current().get_id()
}

#[wrap_syscall]
pub fn getpid() -> usize {
    Scheduler::get_current().get_process().get_pid()
}

#[wrap_syscall]
pub fn getppid() -> usize {
    Scheduler::get_current()
        .get_process()
        .get_parent()
        .map_or(0, |x| x.get_pid())
}

#[wrap_syscall]
pub fn getuid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().user_id as _
}

#[wrap_syscall]
pub fn geteuid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().effective_user_id as _
}

#[wrap_syscall]
pub fn getgid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().group_id as _
}

#[wrap_syscall]
pub fn getegid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().effective_group_id as _
}

#[wrap_syscall]
pub fn getresuid(ruid: VirtAddr, euid: VirtAddr, suid: VirtAddr) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let ident = proc.identity.lock();

    let mut ruid_ptr = UserPtr::<uid_t>::new(ruid);
    let mut euid_ptr = UserPtr::<uid_t>::new(euid);
    let mut suid_ptr = UserPtr::<uid_t>::new(suid);

    ruid_ptr.write(ident.user_id).ok_or(Errno::EFAULT)?;
    euid_ptr
        .write(ident.effective_user_id)
        .ok_or(Errno::EFAULT)?;
    suid_ptr.write(ident.set_user_id).ok_or(Errno::EFAULT)?;

    Ok(())
}

#[wrap_syscall]
pub fn getresgid(rgid: VirtAddr, egid: VirtAddr, sgid: VirtAddr) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let ident = proc.identity.lock();

    let mut rgid_ptr = UserPtr::<uid_t>::new(rgid);
    let mut egid_ptr = UserPtr::<uid_t>::new(egid);
    let mut sgid_ptr = UserPtr::<uid_t>::new(sgid);

    rgid_ptr.write(ident.group_id).ok_or(Errno::EFAULT)?;
    egid_ptr
        .write(ident.effective_group_id)
        .ok_or(Errno::EFAULT)?;
    sgid_ptr.write(ident.set_group_id).ok_or(Errno::EFAULT)?;

    Ok(())
}

#[wrap_syscall]
pub fn getpgid(pid: usize) -> EResult<usize> {
    let proc = if pid == 0 {
        Scheduler::get_current().get_process()
    } else {
        let table = crate::process::PROCESS_TABLE.lock();
        table
            .get(&pid)
            .cloned()
            .ok_or(Errno::ESRCH)?
            .upgrade()
            .ok_or(Errno::ESRCH)?
    };
    Ok(*proc.pgrp.lock())
}

#[wrap_syscall]
pub fn setpgid(pid: usize, pgid: usize) -> EResult<usize> {
    let current = Scheduler::get_current().get_process();
    let target = if pid == 0 {
        current.clone()
    } else {
        let table = crate::process::PROCESS_TABLE.lock();
        table
            .get(&pid)
            .cloned()
            .ok_or(Errno::ESRCH)?
            .upgrade()
            .ok_or(Errno::ESRCH)?
    };

    // Can only set pgid on self or own children.
    if target.get_pid() != current.get_pid() {
        let is_child = current
            .children
            .lock()
            .iter()
            .any(|c| c.get_pid() == target.get_pid());
        if !is_child {
            return Err(Errno::ESRCH);
        }
        // Child must be in the same session.
        if *target.session.lock() != *current.session.lock() {
            return Err(Errno::EPERM);
        }
    }

    let new_pgid = if pgid == 0 { target.get_pid() } else { pgid };

    *target.pgrp.lock() = new_pgid;
    Ok(0)
}

#[wrap_syscall]
pub fn getsid(pid: usize) -> EResult<usize> {
    let proc = if pid == 0 {
        Scheduler::get_current().get_process()
    } else {
        let table = crate::process::PROCESS_TABLE.lock();
        table
            .get(&pid)
            .cloned()
            .ok_or(Errno::ESRCH)?
            .upgrade()
            .ok_or(Errno::ESRCH)?
    };
    Ok(*proc.session.lock())
}

#[wrap_syscall]
pub fn setsid() -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let pid = proc.get_pid();

    // Fail if the process is already a process group leader.
    if *proc.pgrp.lock() == pid {
        // Check that we're also kind of a session leader already — in that case EPERM.
        // POSIX: setsid() fails if the calling process is already a process group leader.
        // However, init (pid 1) is always a pgrp leader, so allow it the first time.
        if *proc.session.lock() == pid {
            return Err(Errno::EPERM);
        }
    }

    *proc.pgrp.lock() = pid;
    *proc.session.lock() = pid;
    *proc.controlling_tty.lock() = None;
    Ok(pid)
}

pub fn exit(error: usize) -> ! {
    Process::exit(error as _);
}

pub fn fork(ctx: &Context) -> EResult<usize> {
    let old = Scheduler::get_current().get_process();

    // Fork the current process. This puts both processes at this point in code.
    let (new_proc, new_task) = old.fork(ctx)?;
    Scheduler::add_task_to_best_cpu(new_task.clone());

    Ok(new_proc.get_pid())
}

#[wrap_syscall]
pub fn execve(path: VirtAddr, argv: VirtAddr, envp: VirtAddr) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let path_str = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let argv_ptr = UserPtr::<usize>::new(argv);
    let envp_ptr = UserPtr::<usize>::new(envp);

    let mut args: Vec<Vec<u8>> = Vec::new();
    let mut envs: Vec<Vec<u8>> = Vec::new();

    for i in 0.. {
        let arg_ptr = VirtAddr::new(argv_ptr.offset(i).read().ok_or(Errno::EFAULT)?);
        if arg_ptr.is_null() {
            break;
        }
        let arg = UserCStr::new(arg_ptr)
            .as_vec(PATH_MAX)
            .ok_or(Errno::EFAULT)?;
        args.push(arg);
    }

    for i in 0.. {
        let env_ptr = VirtAddr::new(envp_ptr.offset(i).read().ok_or(Errno::EFAULT)?);
        if env_ptr.is_null() {
            break;
        }
        let env = UserCStr::new(env_ptr)
            .as_vec(PATH_MAX)
            .ok_or(Errno::EFAULT)?;

        envs.push(env);
    }

    let file = File::open(
        proc.root_dir.lock().clone(),
        proc.working_dir.lock().clone(),
        &path_str,
        OpenFlags::Read | OpenFlags::Executable,
        Mode::empty(),
        &proc.identity.lock(),
    )?;
    proc.fexecve(file, args, envs)?;

    unreachable!("fexecve should never return on success");
}

fn waitpid_matches(pid: uapi::pid_t, caller_pgrp: usize, child: &Process) -> bool {
    match pid as isize {
        p if p > 0 => child.get_pid() == pid,
        -1 => true,
        0 => *child.pgrp.lock() == caller_pgrp,
        p => *child.pgrp.lock() == (-p) as usize,
    }
}

fn encode_exit(code: u8) -> i32 {
    (code as i32) << 8
}

fn encode_stopped(sig: u32) -> i32 {
    0x7f | ((sig as i32) << 8)
}

#[wrap_syscall]
pub fn waitpid(pid: uapi::pid_t, stat_loc: VirtAddr, options: i32) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let caller_pgrp = *proc.pgrp.lock();
    let mut stat_ptr: UserPtr<i32> = UserPtr::new(stat_loc);

    let write_status = |p: &mut UserPtr<i32>, s: i32| -> EResult<()> {
        if stat_loc.is_null() {
            Ok(())
        } else {
            p.write(s).ok_or(Errno::EFAULT).map(|_| ())
        }
    };

    loop {
        let guard = proc.child_event.guard();
        let mut children = proc.children.lock();

        if children.is_empty() {
            return Err(Errno::ECHILD);
        }

        let mut saw_match = false;
        let mut reap: Option<(usize, usize, i32)> = None;
        let mut report: Option<(usize, i32)> = None;

        for (idx, child) in children.iter().enumerate() {
            if !waitpid_matches(pid, caller_pgrp, child) {
                continue;
            }
            saw_match = true;

            let state = child.status.lock();
            match *state {
                ProcessState::Exited(code) => {
                    reap = Some((idx, child.get_pid(), encode_exit(code)));
                    break;
                }
                ProcessState::Stopped(sig)
                    if (options & uapi::wait::WUNTRACED) != 0
                        && child.stop_unwaited.swap(false, Ordering::AcqRel) =>
                {
                    report = Some((child.get_pid(), encode_stopped(sig.as_raw())));
                    break;
                }
                _ if (options & uapi::wait::WCONTINUED) != 0
                    && child.continue_unwaited.swap(false, Ordering::AcqRel) =>
                {
                    report = Some((child.get_pid(), 0xffff));
                    break;
                }
                _ => {}
            }
        }

        if let Some((idx, child_pid, status)) = reap {
            write_status(&mut stat_ptr, status)?;
            children.remove(idx);
            return Ok(child_pid);
        }

        if let Some((child_pid, status)) = report {
            write_status(&mut stat_ptr, status)?;
            return Ok(child_pid);
        }

        if !saw_match {
            return Err(Errno::ECHILD);
        }

        if (options & uapi::wait::WNOHANG) != 0 {
            return Ok(0);
        }

        drop(children);
        guard.wait();

        // On wakeup, re-scan the child list first. Only surface EINTR if the
        // rescan turns up nothing and we have a pending signal.
        // Otherwise a SIGCHLD arriving in parallel with a child transition would race the legitimate reap and steal it.
        if Scheduler::get_current().has_pending_signals() {
            let children = proc.children.lock();
            let any_ready = children.iter().any(|child| {
                if !waitpid_matches(pid, caller_pgrp, child) {
                    return false;
                }
                let state = child.status.lock();
                matches!(*state, ProcessState::Exited(_))
                    || ((options & uapi::wait::WUNTRACED) != 0
                        && child.stop_unwaited.load(Ordering::Acquire))
                    || ((options & uapi::wait::WCONTINUED) != 0
                        && child.continue_unwaited.load(Ordering::Acquire))
            });
            drop(children);
            if !any_ready {
                return Err(Errno::EINTR);
            }
        }
    }
}

const THREAD_NAME_MAX: usize = 16;

#[wrap_syscall]
pub fn thread_create(entry: usize, stack: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let task = Arc::new(crate::process::task::Task::new(
        to_user, entry, stack, &proc, true,
    )?);
    let tid = task.get_id();
    proc.threads.lock().push(task.clone());
    Scheduler::add_task_to_best_cpu(task);
    Ok(tid)
}

pub fn thread_exit() -> ! {
    let task = Scheduler::get_current();
    let proc = task.get_process();
    let tid = task.get_id();

    let last_thread = {
        let mut threads = proc.threads.lock();
        threads.retain(|t| t.get_id() != tid);
        threads.is_empty()
    };

    if last_thread {
        Process::exit(0);
    }

    Scheduler::kill_current();
}

#[wrap_syscall]
pub fn thread_kill(pid: usize, tid: usize, sig: usize) -> EResult<usize> {
    let sig_num = sig as u32;

    // Signal 0 is used to check existence without sending.
    if sig_num != 0 {
        let _ = Signal::from_raw(sig_num).ok_or(Errno::EINVAL)?;
    }

    let target_proc = {
        let table = PROCESS_TABLE.lock();
        table
            .get(&pid)
            .cloned()
            .ok_or(Errno::ESRCH)?
            .upgrade()
            .ok_or(Errno::ESRCH)?
    };

    let thread = {
        let threads = target_proc.threads.lock();
        threads
            .iter()
            .find(|t| t.get_id() == tid)
            .cloned()
            .ok_or(Errno::ESRCH)?
    };

    if sig_num != 0 {
        let sig = Signal::from_raw(sig_num).unwrap();
        signal::send_signal_to_thread(&thread, sig);
    }

    Ok(0)
}

#[wrap_syscall]
pub fn thread_setname(tid: usize, name_ptr: VirtAddr) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let thread = {
        let threads = proc.threads.lock();
        threads
            .iter()
            .find(|t| t.get_id() == tid)
            .cloned()
            .ok_or(Errno::ESRCH)?
    };

    let name_bytes = UserCStr::new(name_ptr)
        .as_vec(THREAD_NAME_MAX)
        .ok_or(Errno::EFAULT)?;
    let name = String::from_utf8(name_bytes).map_err(|_| Errno::EINVAL)?;
    *thread.name.lock() = name;
    Ok(0)
}

#[wrap_syscall]
pub fn umask(mask: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    // Only the permission bits are meaningful.
    let new_mask = (mask as u32) & 0o777;
    Ok(proc.umask.swap(new_mask, Ordering::Relaxed) as usize)
}

#[wrap_syscall]
pub fn thread_getname(tid: usize, buf: VirtAddr, size: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let thread = {
        let threads = proc.threads.lock();
        threads
            .iter()
            .find(|t| t.get_id() == tid)
            .cloned()
            .ok_or(Errno::ESRCH)?
    };

    let name = thread.name.lock();
    // Need space for the name plus a null terminator.
    let required = name.len() + 1;
    if size < required {
        return Err(Errno::ERANGE);
    }

    let mut name_buf: Vec<u8> = Vec::with_capacity(required);
    name_buf.extend_from_slice(name.as_bytes());
    name_buf.push(0);

    if !arch::virt::copy_to_user(buf, &name_buf) {
        return Err(Errno::EFAULT);
    }

    Ok(0)
}
