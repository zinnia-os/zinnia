use core::sync::atomic::Ordering;

use crate::{
    arch::{self, sched::Context},
    memory::{UserCStr, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::{
        Process, State, pgrp_in_session,
        signal::{self, Signal},
        to_user,
    },
    sched::Scheduler,
    uapi::{self, gid_t, limits::PATH_MAX, pid_t, resource::*, uid_t},
    vfs::{File, file::OpenFlags, inode::Mode},
    wrap_syscall,
};
use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

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
pub fn getgroups(size: i32, list: VirtAddr) -> EResult<usize> {
    if size < 0 {
        return Err(Errno::EINVAL);
    }
    let proc = Scheduler::get_current().get_process();
    let groups = proc.identity.lock().groups.clone();
    if size == 0 {
        return Ok(groups.len());
    }
    if (size as usize) < groups.len() {
        return Err(Errno::EINVAL);
    }
    UserPtr::<gid_t>::new(list)
        .write_slice(&groups)
        .ok_or(Errno::EFAULT)?;
    Ok(groups.len())
}

#[wrap_syscall]
pub fn setgroups(size: usize, list: VirtAddr) -> EResult<()> {
    if size > uapi::sysconf::NGROUPS_MAX as usize {
        return Err(Errno::EINVAL);
    }
    let proc = Scheduler::get_current().get_process();
    if !proc.identity.lock().is_effective_superuser() {
        return Err(Errno::EPERM);
    }
    let mut groups = alloc::vec![0 as gid_t; size];
    if size != 0 {
        UserPtr::<gid_t>::new(list)
            .read_slice(&mut groups)
            .ok_or(Errno::EFAULT)?;
    }
    proc.identity.lock().groups = groups;
    Ok(())
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
    let proc = Process::lookup_or_self(pid)?;
    Ok(*proc.pgrp.lock())
}

#[wrap_syscall]
pub fn setpgid(pid: pid_t, pgid: pid_t) -> EResult<pid_t> {
    if pgid < 0 {
        return Err(Errno::EINVAL);
    }

    let current = Scheduler::get_current().get_process();
    let target = Process::lookup_or_self(pid)?;

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
        if target.has_execed.load(Ordering::Acquire) {
            return Err(Errno::EACCES);
        }
    }

    let session = *target.session.lock();
    if session == target.get_pid() {
        return Err(Errno::EPERM);
    }

    let new_pgid = if pgid == 0 { target.get_pid() } else { pgid };

    if new_pgid != target.get_pid() && !pgrp_in_session(new_pgid, session) {
        return Err(Errno::EPERM);
    }

    *target.pgrp.lock() = new_pgid;
    Ok(0)
}

#[wrap_syscall]
pub fn getsid(pid: pid_t) -> EResult<pid_t> {
    let proc = Process::lookup_or_self(pid)?;
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

fn parse_processes(which: u32, who: pid_t) -> EResult<Vec<Arc<Process>>> {
    let current = Scheduler::get_current().get_process();

    if which == PRIO_PROCESS {
        let proc = if who == 0 {
            current
        } else {
            let table = crate::process::PROCESS_TABLE.lock();

            table
                .get(&who)
                .cloned()
                .ok_or(Errno::ESRCH)?
                .upgrade()
                .ok_or(Errno::ESRCH)?
        };

        return Ok(vec![proc]);
    }

    let table = crate::process::PROCESS_TABLE
        .lock()
        .values()
        .filter_map(Weak::upgrade)
        .collect::<Vec<_>>();

    let procs = match which {
        PRIO_PGRP => {
            let pgrp = if who == 0 { *current.pgrp.lock() } else { who };

            table
                .into_iter()
                .filter(|proc| *proc.pgrp.lock() == pgrp)
                .collect::<Vec<_>>()
        }

        PRIO_USER => {
            let uid = if who == 0 {
                current.identity.lock().user_id
            } else {
                who as uid_t
            };

            table
                .into_iter()
                .filter(|proc| proc.identity.lock().user_id == uid)
                .collect::<Vec<_>>()
        }

        _ => return Err(Errno::EINVAL),
    };

    if procs.is_empty() {
        return Err(Errno::ESRCH);
    }

    Ok(procs)
}

#[wrap_syscall]
pub fn getpriority(which: u32, who: pid_t) -> EResult<i32> {
    Ok(parse_processes(which, who)?
        .iter()
        .map(|p| p.get_nice() as i32)
        .min()
        .unwrap())
}

#[wrap_syscall]
pub fn setpriority(which: u32, who: pid_t, prio: i32) -> EResult<()> {
    let procs = parse_processes(which, who)?;

    let nice = Process::clamp_nice(prio);

    let euid = Scheduler::get_current()
        .get_process()
        .identity
        .lock()
        .effective_user_id;

    let mut last_result = Ok(());

    for proc in procs {
        let identity = proc.identity.lock();

        if euid != 0 && euid != identity.user_id && euid != identity.effective_user_id {
            last_result = Err(Errno::EPERM);
            continue;
        }

        let current_nice = proc.get_nice();

        if nice < current_nice && euid != 0 {
            last_result = Err(Errno::EACCES);
            continue;
        }

        proc.set_nice(nice as i32);
    }

    last_result
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

fn read_string_array(array: VirtAddr) -> EResult<Vec<Vec<u8>>> {
    let array_ptr = UserPtr::<usize>::new(array);
    let mut result: Vec<Vec<u8>> = Vec::new();

    for i in 0.. {
        let entry = VirtAddr::new(array_ptr.offset(i).read().ok_or(Errno::EFAULT)?);
        if entry.is_null() {
            break;
        }
        result.push(
            UserCStr::new(entry)
                .as_vec(uapi::limits::ARG_MAX)
                .ok_or(Errno::EFAULT)?,
        );
    }

    Ok(result)
}

#[wrap_syscall]
pub fn execve(path: VirtAddr, argv: VirtAddr, envp: VirtAddr) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let path_str = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;

    let args = read_string_array(argv)?;
    let envs = read_string_array(envp)?;

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

#[wrap_syscall]
pub fn fexecve(fd: i32, argv: VirtAddr, envp: VirtAddr) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let file = proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?.file;

    if !file.flags.lock().contains(OpenFlags::Read) {
        return Err(Errno::EBADF);
    }

    let args = read_string_array(argv)?;
    let envs = read_string_array(envp)?;

    let exec_path = args.first().cloned().unwrap_or_default();

    proc.fexecve(file, exec_path, args, envs)?;

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

enum WaitEvent {
    Exited { pid: pid_t, uid: uid_t, code: u8 },
    Signaled { pid: pid_t, uid: uid_t, sig: Signal },
    Stopped { pid: pid_t, uid: uid_t, sig: Signal },
    Continued { pid: pid_t, uid: uid_t },
}

impl WaitEvent {
    fn pid(&self) -> pid_t {
        match *self {
            Self::Exited { pid, .. }
            | Self::Signaled { pid, .. }
            | Self::Stopped { pid, .. }
            | Self::Continued { pid, .. } => pid,
        }
    }

    fn encode_waitpid(&self) -> i32 {
        match *self {
            Self::Exited { code, .. } => (code as i32) << 8,
            Self::Signaled { sig, .. } => sig as i32,
            Self::Stopped { sig, .. } => 0x7f | ((sig as i32) << 8),
            Self::Continued { .. } => 0xffff,
        }
    }

    fn reaps(&self) -> bool {
        matches!(self, Self::Exited { .. } | Self::Signaled { .. })
    }
}

struct WaitFilter {
    selector: pid_t,
    exited: bool,
    stopped: bool,
    continued: bool,
    nohang: bool,
    nowait: bool,
}

fn wait_for_child(
    proc: &Arc<Process>,
    filter: &WaitFilter,
    mut deliver: impl FnMut(&WaitEvent) -> EResult<()>,
) -> EResult<Option<WaitEvent>> {
    let caller_pgrp = *proc.pgrp.lock();

    loop {
        let guard = proc.child_event.guard();
        {
            let mut children = proc.children.lock();
            if children.is_empty() {
                return Err(Errno::ECHILD);
            }

            let mut saw_match = false;
            let mut hit: Option<(usize, WaitEvent)> = None;

            for (idx, child) in children.iter().enumerate() {
                if !waitpid_matches(filter.selector, caller_pgrp, child) {
                    continue;
                }
                saw_match = true;

                let pid = child.get_pid();
                let uid = child.identity.lock().user_id;
                let state = child.status.lock();
                match *state {
                    State::Exited(code) if filter.exited => {
                        hit = Some((idx, WaitEvent::Exited { pid, uid, code }));
                    }
                    State::Signaled(sig) if filter.exited => {
                        hit = Some((idx, WaitEvent::Signaled { pid, uid, sig }));
                    }
                    State::Stopped(sig)
                        if filter.stopped && child.stop_unwaited.load(Ordering::Acquire) =>
                    {
                        hit = Some((idx, WaitEvent::Stopped { pid, uid, sig }));
                    }
                    _ if filter.continued && child.continue_unwaited.load(Ordering::Acquire) => {
                        hit = Some((idx, WaitEvent::Continued { pid, uid }));
                    }
                    _ => continue,
                }
                break;
            }

            if let Some((idx, event)) = hit {
                deliver(&event)?;
                if !filter.nowait {
                    match event {
                        WaitEvent::Stopped { .. } => {
                            children[idx].stop_unwaited.store(false, Ordering::Release);
                        }
                        WaitEvent::Continued { .. } => {
                            children[idx]
                                .continue_unwaited
                                .store(false, Ordering::Release);
                        }
                        _ => {}
                    }
                    if event.reaps() {
                        children.remove(idx);
                    }
                }
                return Ok(Some(event));
            }

            if !saw_match {
                return Err(Errno::ECHILD);
            }

            if filter.nohang {
                return Ok(None);
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

#[wrap_syscall]
pub fn waitpid(
    pid: pid_t,
    stat_loc: VirtAddr,
    options: i32,
    rusage_loc: VirtAddr,
) -> EResult<pid_t> {
    let proc = Scheduler::get_current().get_process();

    let filter = WaitFilter {
        selector: pid,
        exited: true,
        stopped: (options & uapi::wait::WUNTRACED) != 0,
        continued: (options & uapi::wait::WCONTINUED) != 0,
        nohang: (options & uapi::wait::WNOHANG) != 0,
        nowait: false,
    };

    let deliver = |event: &WaitEvent| -> EResult<()> {
        if !stat_loc.is_null() {
            UserPtr::<i32>::new(stat_loc)
                .write(event.encode_waitpid())
                .ok_or(Errno::EFAULT)?;
        }
        if !rusage_loc.is_null() {
            UserPtr::<uapi::resource::rusage>::new(rusage_loc)
                .write(uapi::resource::rusage::default())
                .ok_or(Errno::EFAULT)?;
        }
        Ok(())
    };

    Ok(wait_for_child(&proc, &filter, deliver)?.map_or(0, |event| event.pid()))
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

    let filter = WaitFilter {
        selector,
        exited: (options & WEXITED) != 0,
        stopped: (options & WSTOPPED) != 0,
        continued: (options & WCONTINUED) != 0,
        nohang: (options & WNOHANG) != 0,
        nowait: (options & WNOWAIT) != 0,
    };

    let write_info = |signo: i32, code: i32, pid: pid_t, uid: uid_t, status: i32| -> EResult<()> {
        if info_loc.is_null() {
            return Ok(());
        }
        let info = siginfo_t {
            si_signo: signo,
            si_code: code,
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

    let deliver = |event: &WaitEvent| -> EResult<()> {
        let (code, pid, uid, status) = match *event {
            WaitEvent::Exited { pid, uid, code } => (CLD_EXITED, pid, uid, code as i32),
            WaitEvent::Signaled { pid, uid, sig } => (CLD_KILLED, pid, uid, sig as i32),
            WaitEvent::Stopped { pid, uid, sig } => (CLD_STOPPED, pid, uid, sig as i32),
            WaitEvent::Continued { pid, uid } => (CLD_CONTINUED, pid, uid, SIGCONT as i32),
        };
        write_info(SIGCHLD as i32, code as i32, pid, uid, status)
    };

    if wait_for_child(&proc, &filter, deliver)?.is_none() {
        write_info(0, 0, 0, 0, 0)?;
    }
    Ok(0)
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
    let last_thread = {
        let task = Scheduler::get_current();
        let proc = task.get_process();
        let tid = task.get_id();

        let mut threads = proc.threads.lock();
        threads.retain(|t| t.get_id() != tid);
        threads.is_empty()
    };

    if last_thread {
        Process::exit(State::Exited(0));
    }
    Scheduler::kill_current();
}

#[wrap_syscall]
pub fn thread_kill(pid: pid_t, tid: usize, sig: u32) -> EResult<pid_t> {
    let sig = match sig {
        0 => None,
        num => Some(Signal::try_from(num).map_err(|_| Errno::EINVAL)?),
    };

    let thread = Process::lookup(pid)?.find_thread(tid)?;

    if let Some(sig) = sig {
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
    let thread = proc.find_thread(tid)?;

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
    let thread = proc.find_thread(tid)?;

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
