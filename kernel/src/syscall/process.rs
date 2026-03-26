use crate::{
    arch::sched::Context,
    memory::{VirtAddr, user::UserPtr},
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::ProcessState,
    sched::Scheduler,
    uapi,
    vfs::{File, file::OpenFlags, inode::Mode},
};
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

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

    let path_str = unsafe { CStr::from_ptr(path.as_ptr()) };

    let args: Vec<_> = (0..)
        .map(|i| unsafe { argv.as_ptr::<usize>().offset(i).read() })
        .take_while(|&p| p != 0)
        .map(|p| {
            unsafe { CStr::from_ptr(p as *const c_char) }
                .to_bytes()
                .to_vec()
        })
        .collect();

    let envs: Vec<_> = (0..)
        .map(|i| unsafe { envp.as_ptr::<usize>().offset(i).read() })
        .take_while(|&p| p != 0)
        .map(|p| {
            unsafe { CStr::from_ptr(p as *const c_char) }
                .to_bytes()
                .to_vec()
        })
        .collect();

    let file = File::open(
        proc.root_dir.lock().clone(),
        proc.working_dir.lock().clone(),
        path_str.to_bytes(),
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
