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
};
use alloc::{string::String, sync::Arc, vec::Vec};

pub fn gettid() -> usize {
    Scheduler::get_current().get_id()
}

pub fn getpid() -> usize {
    Scheduler::get_current().get_process().get_pid()
}

pub fn getppid() -> usize {
    Scheduler::get_current()
        .get_process()
        .get_parent()
        .map_or(0, |x| x.get_pid())
}

pub fn getuid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().user_id as _
}

pub fn geteuid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().effective_user_id as _
}

pub fn getgid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().group_id as _
}

pub fn getegid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().effective_group_id as _
}

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

const WNOHANG: i32 = 1;

pub fn waitpid(pid: uapi::pid_t, stat_loc: VirtAddr, options: i32) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let mut stat_loc: UserPtr<i32> = UserPtr::new(stat_loc);
    let nohang = (options & WNOHANG) != 0;
    loop {
        let guard = proc.child_event.guard();
        let mut inner = proc.children.lock();
        if inner.is_empty() {
            return Err(Errno::ECHILD);
        }
        match pid as isize {
            // Any child process whose process group ID is equal to the absolute value of pid.
            ..=-2 => {
                let target_pgrp = (-(pid as isize)) as usize;
                let mut waitee = None;
                for (idx, child) in inner.iter().enumerate() {
                    if *child.pgrp.lock() != target_pgrp {
                        continue;
                    }
                    let child_inner = child.status.lock();
                    if let ProcessState::Exited(code) = *child_inner {
                        stat_loc.write((code as i32) << 8).ok_or(Errno::EFAULT)?;
                        waitee = Some((idx, child.get_pid()));
                        break;
                    }
                }

                if let Some((w, child_pid)) = waitee {
                    inner.remove(w);
                    return Ok(child_pid);
                }

                // Check if any child matches the process group at all.
                let has_match = inner.iter().any(|c| *c.pgrp.lock() == target_pgrp);
                if !has_match {
                    return Err(Errno::ECHILD);
                }
            }
            -1 | 0 => {
                let mut waitee = None;
                for (idx, child) in inner.iter().enumerate() {
                    let child_inner = child.status.lock();
                    if let ProcessState::Exited(code) = *child_inner {
                        stat_loc.write((code as i32) << 8).ok_or(Errno::EFAULT)?;
                        waitee = Some((idx, child.get_pid()));
                        break;
                    }
                }

                if let Some((w, child_pid)) = waitee {
                    inner.remove(w);
                    return Ok(child_pid);
                }
            }
            _ => {
                let mut found_child = false;
                let mut waitee = None;
                for (idx, child) in inner.iter().enumerate() {
                    if child.get_pid() != pid {
                        continue;
                    }
                    found_child = true;

                    let child_inner = child.status.lock();
                    if let ProcessState::Exited(code) = *child_inner {
                        stat_loc.write((code as i32) << 8).ok_or(Errno::EFAULT)?;
                        waitee = Some((idx, child.get_pid()));
                    }
                    break;
                }

                if !found_child {
                    return Err(Errno::ECHILD);
                }

                if let Some((w, child_pid)) = waitee {
                    inner.remove(w);
                    return Ok(child_pid);
                }
            }
        }
        if nohang {
            return Ok(0);
        }
        drop(inner);
        guard.wait();
        // Check if we were woken by a signal rather than a child event.
        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::EINTR);
        }
    }
}

/// Maximum thread name length (including null terminator), matching Linux's TASK_COMM_LEN.
const THREAD_NAME_MAX: usize = 16;

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
