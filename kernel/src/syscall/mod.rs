mod memory;
mod numbers;
mod process;
mod system;
mod vfs;

use super::posix::errno::Errno;
use crate::arch::sched::Context;

macro_rules! sys_unimp {
    ($name: literal, $sc: expr) => {{
        warn!("Call to unimplemented syscall {}", $name);
        $sc
    }};
}

/// Executes the syscall as identified by `num`.
/// Returns a tuple of (value, error) to the user. An error code of 0 inidcates success.
/// If the error code is not 0, `value` is not valid and indicates failure.
pub fn dispatch(
    frame: &Context,
    num: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> (usize, usize) {
    // Execute a syscall based on the number.
    // Note that the numbers might not be in order, but grouped logically.
    let result = match num {
        // System control
        numbers::SYSLOG => system::syslog(a0, a1.into(), a2),
        numbers::GETUNAME => system::getuname(a0.into()),
        numbers::SETUNAME => system::setuname(a0.into()),
        numbers::ARCHCTL => system::archctl(a0, a1),
        numbers::REBOOT => system::reboot(a0 as _, a1 as _),

        // Mapped memory
        numbers::MMAP => memory::mmap(a0.into(), a1, a2 as _, a3 as _, a4 as _, a5 as _),
        numbers::MUNMAP => sys_unimp!("munmap", memory::munmap(a0.into(), a1)),
        numbers::MPROTECT => memory::mprotect(a0.into(), a1, a2 as _),
        numbers::MADVISE => sys_unimp!("madvise", Err(Errno::ENOSYS)),

        // Signals
        numbers::SIGPROCMASK => sys_unimp!("sigprocmask", Err(Errno::ENOSYS)),
        numbers::SIGSUSPEND => sys_unimp!("sigsuspend", Err(Errno::ENOSYS)),
        numbers::SIGPENDING => sys_unimp!("sigpending", Err(Errno::ENOSYS)),
        numbers::SIGACTION => sys_unimp!("sigaction", Err(Errno::ENOSYS)),
        numbers::SIGTIMEDWAIT => sys_unimp!("sigtimedwait", Err(Errno::ENOSYS)),
        numbers::SIGALTSTACK => sys_unimp!("sigaltstack", Err(Errno::ENOSYS)),

        // Processes
        numbers::EXIT => process::exit(a0),
        numbers::EXECVE => process::execve(a0.into(), a1.into(), a2.into()),
        numbers::FORK => process::fork(frame),
        numbers::KILL => sys_unimp!("kill", Err(Errno::ENOSYS)),
        numbers::GETTID => Ok(process::gettid()),
        numbers::GETPID => Ok(process::getpid()),
        numbers::GETPPID => Ok(process::getppid()),
        numbers::WAITID => sys_unimp!("waitid", Err(Errno::ENOSYS)),
        numbers::WAITPID => process::waitpid(a0 as _, a1.into(), a2 as _),

        // Threads
        numbers::THREAD_CREATE => sys_unimp!("thread_create", Err(Errno::ENOSYS)),
        numbers::THREAD_KILL => sys_unimp!("thread_kill", Err(Errno::ENOSYS)),
        numbers::THREAD_EXIT => sys_unimp!("thread_exit", Err(Errno::ENOSYS)),
        numbers::THREAD_SETNAME => sys_unimp!("thread_setname", Err(Errno::ENOSYS)),
        numbers::THREAD_GETNAME => sys_unimp!("thread_getname", Err(Errno::ENOSYS)),

        // VFS
        numbers::READ => vfs::read(a0 as _, a1.into(), a2).map(|x| x as _),
        numbers::PREAD => vfs::pread(a0 as _, a1.into(), a2, a3).map(|x| x as _),
        numbers::WRITE => vfs::write(a0 as _, a1.into(), a2).map(|x| x as _),
        numbers::PWRITE => vfs::pwrite(a0 as _, a1.into(), a2, a3).map(|x| x as _),
        numbers::SEEK => vfs::seek(a0 as _, a1, a2),
        numbers::IOCTL => vfs::ioctl(a0 as _, a1, a2.into()),
        numbers::OPENAT => vfs::openat(a0 as _, a1.into(), a2).map(|x| x as _),
        numbers::CLOSE => vfs::close(a0 as _),
        numbers::FSTAT => vfs::fstat(a0 as _, a1.into()),
        numbers::FSTATAT => vfs::fstatat(a0 as _, a1.into(), a2.into(), a3),
        numbers::STATVFS => sys_unimp!("statvfs", Err(Errno::ENOSYS)),
        numbers::FSTATVFS => sys_unimp!("fstatvfs", Err(Errno::ENOSYS)),
        numbers::FACCESSAT => vfs::faccessat(a0 as _, a1.into(), a2, a3).map(|_| 0),
        numbers::FCNTL => vfs::fcntl(a0 as _, a1, a2),
        numbers::FTRUNCATE => sys_unimp!("ftruncate", Err(Errno::ENOSYS)),
        numbers::FALLOCATE => sys_unimp!("fallocate", Err(Errno::ENOSYS)),
        numbers::UTIMENSAT => sys_unimp!("utimensat", Err(Errno::ENOSYS)),
        numbers::PSELECT => vfs::pselect(a0, a1.into(), a2.into(), a3.into(), a4.into(), a5.into()),
        numbers::MKNODAT => sys_unimp!("mknodat", Err(Errno::ENOSYS)),
        numbers::READDIR => sys_unimp!("readdir", Err(Errno::ENOSYS)),
        numbers::GETCWD => vfs::getcwd(a0.into(), a1),
        numbers::CHDIR => vfs::chdir(a0.into()).map(|_| 0),
        numbers::FCHDIR => vfs::fchdir(a0 as _).map(|_| 0),
        numbers::MKDIRAT => vfs::mkdirat(a0 as _, a1.into(), a2 as _).map(|x| x as _),
        numbers::RMDIRAT => sys_unimp!("rmdirat", Err(Errno::ENOSYS)),
        numbers::GETDENTS => vfs::getdents(a0 as _, a1.into(), a2),
        numbers::RENAMEAT => sys_unimp!("renameat", Err(Errno::ENOSYS)),
        numbers::FCHMOD => sys_unimp!("fchmod", Err(Errno::ENOSYS)),
        numbers::FCHMODAT => sys_unimp!("fchmodat", Err(Errno::ENOSYS)),
        numbers::FCHOWNAT => sys_unimp!("fchownat", Err(Errno::ENOSYS)),
        numbers::LINKAT => vfs::linkat(a0 as _, a1.into(), a2 as _, a3.into(), a4 as _).map(|_| 0),
        numbers::SYMLINKAT => sys_unimp!("symlinkat", Err(Errno::ENOSYS)),
        numbers::UNLINKAT => vfs::unlinkat(a0 as _, a1.into(), a2).map(|_| 0),
        numbers::READLINKAT => sys_unimp!("readlinkat", Err(Errno::ENOSYS)),
        numbers::FLOCK => sys_unimp!("flock", Err(Errno::ENOSYS)),
        numbers::PPOLL => vfs::ppoll(a0.into(), a1, a2.into(), a3.into()),
        numbers::DUP => vfs::dup(a0 as _).map(|x| x as _),
        numbers::DUP3 => vfs::dup3(a0 as _, a1 as _, a2).map(|x| x as _),
        numbers::SYNC => sys_unimp!("sync", Err(Errno::ENOSYS)),
        numbers::FSYNC => sys_unimp!("fsync", Err(Errno::ENOSYS)),
        numbers::FDATASYNC => sys_unimp!("fdatasync", Err(Errno::ENOSYS)),
        numbers::CHROOT => sys_unimp!("chroot", Err(Errno::ENOSYS)),
        numbers::MOUNT => sys_unimp!("mount", Err(Errno::ENOSYS)),
        numbers::UMOUNT => sys_unimp!("umount", Err(Errno::ENOSYS)),
        numbers::PIPE => vfs::pipe(a0.into()),

        // Sockets
        numbers::SOCKET => sys_unimp!("socket", Err(Errno::ENOSYS)),
        numbers::SOCKETPAIR => sys_unimp!("socketpair", Err(Errno::ENOSYS)),
        numbers::SHUTDOWN => sys_unimp!("shutdown", Err(Errno::ENOSYS)),
        numbers::BIND => sys_unimp!("bind", Err(Errno::ENOSYS)),
        numbers::CONNECT => sys_unimp!("connect", Err(Errno::ENOSYS)),
        numbers::ACCEPT => sys_unimp!("accept", Err(Errno::ENOSYS)),
        numbers::LISTEN => sys_unimp!("listen", Err(Errno::ENOSYS)),
        numbers::GETPEERNAME => sys_unimp!("getpeername", Err(Errno::ENOSYS)),
        numbers::GETSOCKNAME => sys_unimp!("getsockname", Err(Errno::ENOSYS)),
        numbers::GETSOCKOPT => sys_unimp!("getsockopt", Err(Errno::ENOSYS)),
        numbers::SETSOCKOPT => sys_unimp!("setsockopt", Err(Errno::ENOSYS)),
        numbers::SENDMSG => sys_unimp!("sendmsg", Err(Errno::ENOSYS)),
        numbers::SENDTO => sys_unimp!("sendto", Err(Errno::ENOSYS)),
        numbers::RECVMSG => sys_unimp!("recvmsg", Err(Errno::ENOSYS)),
        numbers::RECVFROM => sys_unimp!("recvfrom", Err(Errno::ENOSYS)),

        // Identity
        numbers::GETGROUPS => sys_unimp!("getgroups", Ok(0)),
        numbers::SETGROUPS => sys_unimp!("setgroups", Err(Errno::ENOSYS)),
        numbers::GETSID => sys_unimp!("getsid", Err(Errno::ENOSYS)),
        numbers::SETSID => sys_unimp!("setsid", Ok(0)),
        numbers::SETUID => sys_unimp!("setuid", Err(Errno::ENOSYS)),
        numbers::GETUID => Ok(process::getuid()),
        numbers::SETGID => sys_unimp!("setgid", Err(Errno::ENOSYS)),
        numbers::GETGID => Ok(process::getgid()),
        numbers::GETEUID => Ok(process::geteuid()),
        numbers::SETEUID => sys_unimp!("seteuid", Err(Errno::ENOSYS)),
        numbers::GETEGID => Ok(process::getegid()),
        numbers::SETEGID => sys_unimp!("setegid", Err(Errno::ENOSYS)),
        numbers::GETPGID => process::getpgid(a0),
        numbers::SETPGID => sys_unimp!("setpgid", Err(Errno::ENOSYS)),
        numbers::GETRESUID => sys_unimp!("getresuid", Err(Errno::ENOSYS)),
        numbers::SETRESUID => sys_unimp!("setresuid", Err(Errno::ENOSYS)),
        numbers::GETRESGID => sys_unimp!("getresgid", Err(Errno::ENOSYS)),
        numbers::SETRESGID => sys_unimp!("setresgid", Err(Errno::ENOSYS)),
        numbers::SETREUID => sys_unimp!("setreuid", Err(Errno::ENOSYS)),
        numbers::SETREGID => sys_unimp!("setregid", Err(Errno::ENOSYS)),
        numbers::UMASK => sys_unimp!("umask", Err(Errno::ENOSYS)),

        // Limits
        numbers::GETRUSAGE => sys_unimp!("getrusage", Err(Errno::ENOSYS)),
        numbers::GETRLIMIT => sys_unimp!("getrlimit", Err(Errno::ENOSYS)),
        numbers::SETRLIMIT => sys_unimp!("setrlimit", Err(Errno::ENOSYS)),

        // Futexes
        numbers::FUTEX_WAIT => sys_unimp!("futex_wait", Ok(0)),
        numbers::FUTEX_WAKE => sys_unimp!("futex_wake", Ok(0)),

        // Time
        numbers::TIMER_CREATE => sys_unimp!("timer_create", Ok(0)),
        numbers::TIMER_SET => sys_unimp!("timer_set", Err(Errno::ENOSYS)),
        numbers::TIMER_DELETE => sys_unimp!("timer_delete", Err(Errno::ENOSYS)),
        numbers::ITIMER_GET => sys_unimp!("itimer_get", Err(Errno::ENOSYS)),
        numbers::ITIMER_SET => sys_unimp!("itimer_set", Err(Errno::ENOSYS)),
        numbers::CLOCK_GET => system::clock_get(a0 as _, a1.into()),
        numbers::CLOCK_GETRES => sys_unimp!("clock_getres", Err(Errno::ENOSYS)),

        // Scheduling
        numbers::SLEEP => system::sleep(a0.into(), a1.into()),
        numbers::YIELD => sys_unimp!("yield", Ok(0)),
        numbers::GETPRIORITY => sys_unimp!("getpriority", Err(Errno::ENOSYS)),
        numbers::SETPRIORITY => sys_unimp!("setpriority", Err(Errno::ENOSYS)),
        numbers::SCHED_GETPARAM => sys_unimp!("sched_getparam", Err(Errno::ENOSYS)),
        numbers::SCHED_SETPARAM => sys_unimp!("sched_setparam", Err(Errno::ENOSYS)),
        numbers::GETENTROPY => sys_unimp!("getentropy", Ok(0)),

        _ => {
            warn!("Unknown syscall {num}");
            Err(Errno::ENOSYS)
        }
    };

    match result {
        Ok(x) => return (x, 0),
        Err(x) => return (0, x as usize),
    }
}
