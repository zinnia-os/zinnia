use core::sync::atomic::Ordering;

use crate::{
    arch::{self, sched::Context},
    memory::{UserCStr, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::{
        PROCESS_TABLE, Process, State,
        signal::{self, Signal},
        to_user,
    },
    sched::Scheduler,
    uapi::{self, gid_t, limits::PATH_MAX, pid_t, uid_t},
    vfs::{File, file::OpenFlags, inode::Mode},
    wrap_syscall,
};
use alloc::{string::String, sync::Arc, vec::Vec};

#[wrap_syscall]
pub fn gettid() -> usize {
    Scheduler::get_current().get_id()
}

#[wrap_syscall]
pub fn getpid() -> pid_t {
    Scheduler::get_current().get_process().get_pid()
}

#[wrap_syscall]
pub fn getppid() -> pid_t {
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
pub fn setuid(uid: uid_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut ident = proc.identity.lock();
    if ident.is_effective_superuser() {
        ident.user_id = uid;
        ident.effective_user_id = uid;
        ident.set_user_id = uid;
        Ok(())
    } else if uid == ident.user_id || uid == ident.effective_user_id || uid == ident.set_user_id {
        ident.effective_user_id = uid;
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}

#[wrap_syscall]
pub fn seteuid(euid: uid_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut ident = proc.identity.lock();
    if ident.is_effective_superuser()
        || euid == ident.user_id
        || euid == ident.effective_user_id
        || euid == ident.set_user_id
    {
        ident.effective_user_id = euid;
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}

#[wrap_syscall]
pub fn setresuid(ruid: uid_t, euid: uid_t, suid: uid_t) -> EResult<()> {
    setresuid_inner(ruid, euid, suid)
}

#[wrap_syscall]
pub fn setreuid(ruid: uid_t, euid: uid_t) -> EResult<()> {
    setresuid_inner(ruid, euid, uid_t::MAX)
}

fn setresuid_inner(ruid: uid_t, euid: uid_t, suid: uid_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut ident = proc.identity.lock();
    if !ident.is_effective_superuser() {
        for uid in [ruid, euid, suid] {
            if uid != uid_t::MAX
                && uid != ident.user_id
                && uid != ident.effective_user_id
                && uid != ident.set_user_id
            {
                return Err(Errno::EPERM);
            }
        }
    }
    if ruid != uid_t::MAX {
        ident.user_id = ruid;
    }
    if euid != uid_t::MAX {
        ident.effective_user_id = euid;
    }
    if suid != uid_t::MAX {
        ident.set_user_id = suid;
    }
    Ok(())
}

#[wrap_syscall]
pub fn getresgid(rgid: VirtAddr, egid: VirtAddr, sgid: VirtAddr) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let ident = proc.identity.lock();

    let mut rgid_ptr = UserPtr::<gid_t>::new(rgid);
    let mut egid_ptr = UserPtr::<gid_t>::new(egid);
    let mut sgid_ptr = UserPtr::<gid_t>::new(sgid);

    rgid_ptr.write(ident.group_id).ok_or(Errno::EFAULT)?;
    egid_ptr
        .write(ident.effective_group_id)
        .ok_or(Errno::EFAULT)?;
    sgid_ptr.write(ident.set_group_id).ok_or(Errno::EFAULT)?;

    Ok(())
}

#[wrap_syscall]
pub fn setgid(gid: gid_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut ident = proc.identity.lock();
    if ident.is_effective_superuser() {
        ident.group_id = gid;
        ident.effective_group_id = gid;
        ident.set_group_id = gid;
        Ok(())
    } else if gid == ident.group_id || gid == ident.effective_group_id || gid == ident.set_group_id
    {
        ident.effective_group_id = gid;
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}

#[wrap_syscall]
pub fn setegid(egid: gid_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut ident = proc.identity.lock();
    if ident.is_effective_superuser()
        || egid == ident.group_id
        || egid == ident.effective_group_id
        || egid == ident.set_group_id
    {
        ident.effective_group_id = egid;
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}

#[wrap_syscall]
pub fn setresgid(rgid: gid_t, egid: gid_t, sgid: gid_t) -> EResult<()> {
    setresgid_inner(rgid, egid, sgid)
}

#[wrap_syscall]
pub fn setregid(rgid: gid_t, egid: gid_t) -> EResult<()> {
    setresgid_inner(rgid, egid, gid_t::MAX)
}

fn setresgid_inner(rgid: gid_t, egid: gid_t, sgid: gid_t) -> EResult<()> {
    let proc = Scheduler::get_current().get_process();
    let mut ident = proc.identity.lock();
    if !ident.is_effective_superuser() {
        for gid in [rgid, egid, sgid] {
            if gid != gid_t::MAX
                && gid != ident.group_id
                && gid != ident.effective_group_id
                && gid != ident.set_group_id
            {
                return Err(Errno::EPERM);
            }
        }
    }
    if rgid != gid_t::MAX {
        ident.group_id = rgid;
    }
    if egid != gid_t::MAX {
        ident.effective_group_id = egid;
    }
    if sgid != gid_t::MAX {
        ident.set_group_id = sgid;
    }
    Ok(())
}

#[wrap_syscall]
pub fn getpgid(pid: pid_t) -> EResult<pid_t> {
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
pub fn setpgid(pid: pid_t, pgid: pid_t) -> EResult<pid_t> {
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
pub fn getsid(pid: pid_t) -> EResult<pid_t> {
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
pub fn setsid() -> EResult<pid_t> {
    let proc = Scheduler::get_current().get_process();
    let pid = proc.get_pid();

    // Fail if the process is already a process group leader.
    if *proc.pgrp.lock() == pid {
        // Check that we're also kind of a session leader already. In that case EPERM.
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
    Process::exit(State::Exited(error as _));
}

pub fn fork(ctx: &Context) -> EResult<pid_t> {
    let old = Scheduler::get_current().get_process();

    let (new_proc, new_task) = old.fork(ctx)?;
    let child_pid = new_proc.get_pid();
    Scheduler::add_task_to_best_cpu(new_task.clone());

    Ok(child_pid)
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

    let root = proc.root_dir.lock().clone();
    let cwd = proc.working_dir.lock().clone();
    let identity = proc.identity.lock().clone();
    let file = File::open(
        root,
        cwd,
        &path_str,
        OpenFlags::Read | OpenFlags::Executable,
        Mode::empty(),
        &identity,
    )?;

    proc.fexecve(file, path_str, args, envs)?;

    unreachable!("fexecve should never return on success");
}

fn waitpid_matches(pid: pid_t, caller_pgrp: pid_t, child: &Process) -> bool {
    match pid {
        p if p > 0 => child.get_pid() == pid,
        -1 => true,
        0 => *child.pgrp.lock() == caller_pgrp,
        p => *child.pgrp.lock() == (-p),
    }
}

fn encode_exit(code: u8) -> i32 {
    (code as i32) << 8
}

fn encode_signaled(sig: u32) -> i32 {
    sig as i32
}

fn encode_stopped(sig: u32) -> i32 {
    0x7f | ((sig as i32) << 8)
}

#[wrap_syscall]
pub fn waitpid(pid: pid_t, stat_loc: VirtAddr, options: i32) -> EResult<pid_t> {
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
        {
            let mut children = proc.children.lock();

            if children.is_empty() {
                return Err(Errno::ECHILD);
            }

            let mut saw_match = false;
            let mut reap: Option<(usize, pid_t, i32)> = None;
            let mut report: Option<(pid_t, i32)> = None;

            for (idx, child) in children.iter().enumerate() {
                if !waitpid_matches(pid, caller_pgrp, child) {
                    continue;
                }
                saw_match = true;

                let state = child.status.lock();
                match *state {
                    State::Exited(code) => {
                        reap = Some((idx, child.get_pid(), encode_exit(code)));
                        break;
                    }
                    State::Signaled(sig) => {
                        reap = Some((idx, child.get_pid(), encode_signaled(sig as u32)));
                        break;
                    }
                    State::Stopped(sig)
                        if (options & uapi::wait::WUNTRACED) != 0
                            && child.stop_unwaited.swap(false, Ordering::AcqRel) =>
                    {
                        report = Some((child.get_pid(), encode_stopped(sig as u32)));
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
        }

        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::ERESTART);
        }
        guard.wait();
        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::ERESTART);
        }
    }
}

const P_ALL: i32 = 0;
const P_PID: i32 = 1;
const P_PGID: i32 = 2;

#[wrap_syscall]
pub fn waitid(idtype: i32, id: pid_t, info_loc: VirtAddr, options: i32) -> EResult<usize> {
    use uapi::signal::{
        CLD_CONTINUED, CLD_EXITED, CLD_KILLED, CLD_STOPPED, SIGCHLD, SIGCONT, siginfo_t, sigval,
    };
    use uapi::wait::{WCONTINUED, WEXITED, WNOHANG, WNOWAIT, WSTOPPED};

    let selector: pid_t = match idtype {
        P_ALL => -1,
        P_PID if id > 0 => id,
        P_PGID => -id,
        _ => return Err(Errno::EINVAL),
    };

    // waitid requires at least one event class to be requested.
    if (options & (WEXITED | WSTOPPED | WCONTINUED)) == 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let caller_pgrp = *proc.pgrp.lock();
    let nowait = (options & WNOWAIT) != 0;

    let write_info = |signo: i32, code: u32, pid: pid_t, uid: u32, status: i32| -> EResult<()> {
        if info_loc.is_null() {
            return Ok(());
        }
        let info = siginfo_t {
            si_signo: signo,
            si_code: code as i32,
            si_errno: 0,
            si_pid: pid,
            si_uid: uid,
            si_addr: UserPtr::new(VirtAddr::null()),
            si_status: status,
            si_value: sigval { sival_int: 0 },
        };
        UserPtr::<siginfo_t>::new(info_loc)
            .write(info)
            .ok_or(Errno::EFAULT)
    };

    loop {
        let guard = proc.child_event.guard();
        {
            let mut children = proc.children.lock();
            if children.is_empty() {
                return Err(Errno::ECHILD);
            }

            let mut saw_match = false;
            // (index, reapable, pid, uid, code, status)
            let mut hit: Option<(usize, bool, pid_t, u32, u32, i32)> = None;

            for (idx, child) in children.iter().enumerate() {
                if !waitpid_matches(selector, caller_pgrp, child) {
                    continue;
                }
                saw_match = true;
                let uid = child.identity.lock().user_id as u32;
                let state = child.status.lock();
                match *state {
                    State::Exited(code) if (options & WEXITED) != 0 => {
                        hit = Some((idx, true, child.get_pid(), uid, CLD_EXITED, code as i32));
                        break;
                    }
                    State::Signaled(sig) if (options & WEXITED) != 0 => {
                        hit = Some((idx, true, child.get_pid(), uid, CLD_KILLED, sig as i32));
                        break;
                    }
                    State::Stopped(sig)
                        if (options & WSTOPPED) != 0
                            && (if nowait {
                                child.stop_unwaited.load(Ordering::Acquire)
                            } else {
                                child.stop_unwaited.swap(false, Ordering::AcqRel)
                            }) =>
                    {
                        hit = Some((idx, false, child.get_pid(), uid, CLD_STOPPED, sig as i32));
                        break;
                    }
                    _ if (options & WCONTINUED) != 0
                        && (if nowait {
                            child.continue_unwaited.load(Ordering::Acquire)
                        } else {
                            child.continue_unwaited.swap(false, Ordering::AcqRel)
                        }) =>
                    {
                        hit = Some((
                            idx,
                            false,
                            child.get_pid(),
                            uid,
                            CLD_CONTINUED,
                            SIGCONT as i32,
                        ));
                        break;
                    }
                    _ => {}
                }
            }

            if let Some((idx, reapable, pid, uid, code, status)) = hit {
                write_info(SIGCHLD as i32, code, pid, uid, status)?;
                if reapable && !nowait {
                    children.remove(idx);
                }
                return Ok(0);
            }

            if !saw_match {
                return Err(Errno::ECHILD);
            }

            // Nothing waitable yet.
            if (options & WNOHANG) != 0 {
                write_info(0, 0, 0, 0, 0)?;
                return Ok(0);
            }
        }

        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::ERESTART);
        }
        guard.wait();
        if Scheduler::get_current().has_pending_signals() {
            return Err(Errno::ERESTART);
        }
    }
}

const THREAD_NAME_MAX: usize = 16;

#[wrap_syscall]
pub fn thread_create(entry: usize, stack: usize) -> EResult<usize> {
    let current = Scheduler::get_current();
    let proc = current.get_process();
    let task = Arc::new(crate::process::task::Task::new(
        to_user, entry, stack, &proc, true,
    )?);
    task.signal.lock().mask = current.signal.lock().mask;
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
        drop(proc);
        drop(task);
        Process::exit(State::Exited(0));
    }

    drop(proc);
    drop(task);
    Scheduler::kill_current();
}

#[wrap_syscall]
pub fn thread_kill(pid: pid_t, tid: usize, sig: u32) -> EResult<pid_t> {
    let sig_num = sig as u32;

    // Signal 0 is used to check existence without sending.
    if sig_num != 0 {
        let _ = Signal::try_from(sig_num).map_err(|_| Errno::EINVAL)?;
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
        let sig = Signal::try_from(sig_num).unwrap();
        let sender = Scheduler::get_current().get_process();
        let mut info = signal::SigInfoData::user(sender.get_pid(), sender.identity.lock().user_id);
        info.code = crate::uapi::signal::SI_TKILL as i32;
        signal::send_signal_info_to_thread(&thread, sig, info);
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
