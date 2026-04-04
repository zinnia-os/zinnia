use crate::{
    arch::sched::Context,
    memory::{UserCStr, VirtAddr, user::UserPtr},
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::ProcessState,
    sched::Scheduler,
    uapi::{self, limits::PATH_MAX},
    vfs::{File, file::OpenFlags, inode::Mode},
};
use alloc::vec::Vec;

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
    proc.identity.lock().user_id
}

pub fn geteuid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().effective_user_id
}

pub fn getgid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().group_id
}

pub fn getegid() -> usize {
    let proc = Scheduler::get_current().get_process();
    proc.identity.lock().effective_group_id
}

pub fn getpgid(pid: usize) -> EResult<usize> {
    if pid != 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    Ok(proc.get_pid())
}

pub fn exit(error: usize) -> ! {
    let proc = Scheduler::get_current().get_process();
    let error = error as i8;

    if proc.get_pid() <= 1 {
        panic!("Attempted to kill init with error code {error}");
    }

    proc.exit(error as _);
    unreachable!();
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

pub fn waitpid(pid: uapi::pid_t, stat_loc: VirtAddr, _options: i32) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    let mut stat_loc: UserPtr<i32> = UserPtr::new(stat_loc);
    loop {
        let mut inner = proc.children.lock();
        if inner.is_empty() {
            return Err(Errno::ECHILD);
        }
        match pid as isize {
            // Any child process whose process group ID is equal to the absolute value of pid.
            ..=-2 => {
                todo!();
            }
            -1 | 0 => {
                let mut waitee = None;
                for (idx, child) in inner.iter().enumerate() {
                    let child_inner = child.status.lock();
                    if let ProcessState::Exited(code) = *child_inner {
                        stat_loc.write((code as i32) << 8).ok_or(Errno::EFAULT)?;
                        waitee = Some(idx);
                    }
                }

                if let Some(w) = waitee {
                    inner.remove(w);
                }
            }
            _ => {
                let mut waitee = None;
                for (idx, child) in inner.iter().enumerate() {
                    if child.get_pid() != pid {
                        continue;
                    }

                    let child_inner = child.status.lock();
                    if let ProcessState::Exited(code) = *child_inner {
                        stat_loc.write((code as i32) << 8).ok_or(Errno::EFAULT)?;
                        waitee = Some(idx);
                    }
                }

                if let Some(w) = waitee {
                    inner.remove(w);
                }
            }
        }
        drop(inner);
        CpuData::get().scheduler.reschedule();
    }
}
