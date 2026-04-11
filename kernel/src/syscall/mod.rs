mod memory;
mod module;
mod numbers;
mod process;
mod signal;
mod socket;
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
    frame: &mut Context,
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
        numbers::SIGPROCMASK => signal::sigprocmask(a0, a1.into(), a2.into()),
        numbers::SIGSUSPEND => sys_unimp!("sigsuspend", Err(Errno::ENOSYS)),
        numbers::SIGPENDING => sys_unimp!("sigpending", Err(Errno::ENOSYS)),
        numbers::SIGACTION => signal::sigaction(a0 as _, a1.into(), a2.into()),
        numbers::SIGTIMEDWAIT => sys_unimp!("sigtimedwait", Err(Errno::ENOSYS)),
        numbers::SIGALTSTACK => sys_unimp!("sigaltstack", Err(Errno::ENOSYS)),
        numbers::SIGRETURN => signal::sigreturn(frame),

        // Processes
        numbers::EXIT => process::exit(a0),
        numbers::EXECVE => process::execve(a0.into(), a1.into(), a2.into()),
        numbers::FORK => process::fork(frame),
        numbers::KILL => signal::kill(a0 as isize, a1),
        numbers::GETTID => Ok(process::gettid()),
        numbers::GETPID => Ok(process::getpid()),
        numbers::GETPPID => Ok(process::getppid()),
        numbers::WAITID => sys_unimp!("waitid", Err(Errno::ENOSYS)),
        numbers::WAITPID => process::waitpid(a0 as _, a1.into(), a2 as _),

        // Threads
        numbers::THREAD_CREATE => process::thread_create(a0, a1),
        numbers::THREAD_KILL => process::thread_kill(a0, a1, a2),
        numbers::THREAD_EXIT => process::thread_exit(),
        numbers::THREAD_SETNAME => process::thread_setname(a0, a1.into()),
        numbers::THREAD_GETNAME => process::thread_getname(a0, a1.into(), a2),

        // VFS
        numbers::PREAD => vfs::pread(a0 as _, a1.into(), a2, a3).map(|x| x as _),
        numbers::READV => vfs::readv(a0 as _, a1.into(), a2).map(|x| x as _),
        numbers::PWRITE => vfs::pwrite(a0 as _, a1.into(), a2, a3).map(|x| x as _),
        numbers::WRITEV => vfs::writev(a0 as _, a1.into(), a2).map(|x| x as _),
        numbers::SEEK => vfs::seek(a0 as _, a1, a2),
        numbers::IOCTL => vfs::ioctl(a0 as _, a1, a2.into()),
        numbers::OPENAT => vfs::openat(a0 as _, a1.into(), a2, a3 as _).map(|x| x as _),
        numbers::CLOSE => vfs::close(a0 as _),
        numbers::FSTAT => vfs::fstat(a0 as _, a1.into()).map(|_| 0),
        numbers::FSTATAT => vfs::fstatat(a0 as _, a1.into(), a2.into(), a3).map(|_| 0),
        numbers::STATVFS => vfs::statvfs(a0.into(), a1.into()).map(|_| 0),
        numbers::FSTATVFS => vfs::fstatvfs(a0 as _, a1.into()).map(|_| 0),
        numbers::FACCESSAT => vfs::faccessat(a0 as _, a1.into(), a2, a3).map(|_| 0),
        numbers::FCNTL => vfs::fcntl(a0 as _, a1, a2),
        numbers::FTRUNCATE => sys_unimp!("ftruncate", Err(Errno::ENOSYS)),
        numbers::FALLOCATE => sys_unimp!("fallocate", Err(Errno::ENOSYS)),
        numbers::UTIMENSAT => sys_unimp!("utimensat", Err(Errno::ENOSYS)),
        numbers::MKNODAT => sys_unimp!("mknodat", Err(Errno::ENOSYS)),
        numbers::GETCWD => vfs::getcwd(a0.into(), a1),
        numbers::CHDIR => vfs::chdir(a0.into()).map(|_| 0),
        numbers::FCHDIR => vfs::fchdir(a0 as _).map(|_| 0),
        numbers::MKDIRAT => vfs::mkdirat(a0 as _, a1.into(), a2 as _).map(|x| x as _),
        numbers::RMDIRAT => sys_unimp!("rmdirat", Err(Errno::ENOSYS)),
        numbers::GETDENTS => vfs::getdents(a0 as _, a1.into(), a2),
        numbers::RENAMEAT => vfs::renameat(a0 as _, a1.into(), a2 as _, a3.into()).map(|_| 0),
        numbers::FCHMOD => vfs::fchmod(a0 as _, a1 as _).map(|_| 0),
        numbers::FCHMODAT => vfs::fchmodat(a0 as _, a1.into(), a2 as _, a3).map(|_| 0),
        numbers::FCHOWNAT => vfs::fchownat(a0 as _, a1.into(), a2 as _, a3 as _, a4).map(|_| 0),
        numbers::LINKAT => vfs::linkat(a0 as _, a1.into(), a2 as _, a3.into(), a4 as _).map(|_| 0),
        numbers::SYMLINKAT => sys_unimp!("symlinkat", Err(Errno::ENOSYS)),
        numbers::UNLINKAT => vfs::unlinkat(a0 as _, a1.into(), a2).map(|_| 0),
        numbers::READLINKAT => {
            vfs::readlinkat(a0 as _, a1.into(), a2.into(), a3 as _).map(|x| x as _)
        }
        numbers::FLOCK => sys_unimp!("flock", Err(Errno::ENOSYS)),
        numbers::PPOLL => vfs::ppoll(a0.into(), a1, a2.into(), a3.into()),
        numbers::DUP => vfs::dup(a0 as _).map(|x| x as _),
        numbers::DUP3 => vfs::dup3(a0 as _, a1 as _, a2).map(|x| x as _),
        numbers::SYNC => sys_unimp!("sync", Err(Errno::ENOSYS)),
        numbers::FSYNC => sys_unimp!("fsync", Err(Errno::ENOSYS)),
        numbers::FDATASYNC => sys_unimp!("fdatasync", Err(Errno::ENOSYS)),
        numbers::CHROOT => vfs::chroot(a0.into()),
        numbers::MOUNT => vfs::mount(a0.into(), a1.into(), a2 as _, a3.into()),
        numbers::UMOUNT => vfs::umount(a0.into(), a1 as _),
        numbers::PIPE => vfs::pipe(a0.into()).map(|_| 0),

        // Sockets
        numbers::SOCKET => socket::socket(a0 as _, a1 as _, a2 as _),
        numbers::SOCKETPAIR => socket::socketpair(a0 as _, a1 as _, a2 as _),
        numbers::SHUTDOWN => socket::shutdown(a0 as _, a1 as _).map(|_| 0),
        numbers::BIND => socket::bind(a0 as _, a1.into(), a2 as _).map(|_| 0),
        numbers::CONNECT => socket::connect(a0 as _, a1.into(), a2 as _).map(|_| 0),
        numbers::ACCEPT => socket::accept(a0 as _, a1.into(), a2.into(), a3.into(), a4).map(|_| 0),
        numbers::LISTEN => socket::listen(a0 as _, a1 as _).map(|_| 0),
        numbers::GETPEERNAME => socket::getpeername(a0 as _, a1.into(), a2.into()).map(|_| 0),
        numbers::GETSOCKNAME => socket::getsockname(a0 as _, a1.into(), a2.into()).map(|_| 0),
        numbers::GETSOCKOPT => {
            socket::getsockopt(a0 as _, a1 as _, a2 as _, a3.into(), a4.into()).map(|_| 0)
        }
        numbers::SETSOCKOPT => {
            socket::setsockopt(a0 as _, a1 as _, a2 as _, a3.into(), a4).map(|_| 0)
        }
        numbers::SENDMSG => socket::sendmsg(a0 as _, a1.into(), a2 as _),
        numbers::RECVMSG => socket::recvmsg(a0 as _, a1.into(), a2 as _),

        // Identity
        numbers::GETGROUPS => sys_unimp!("getgroups", Ok(0)),
        numbers::SETGROUPS => sys_unimp!("setgroups", Err(Errno::ENOSYS)),
        numbers::GETSID => process::getsid(a0),
        numbers::SETSID => process::setsid(),
        numbers::SETUID => sys_unimp!("setuid", Ok(0)),
        numbers::GETUID => Ok(process::getuid()),
        numbers::SETGID => sys_unimp!("setgid", Ok(0)),
        numbers::GETGID => Ok(process::getgid()),
        numbers::GETEUID => Ok(process::geteuid()),
        numbers::SETEUID => sys_unimp!("seteuid", Ok(0)),
        numbers::GETEGID => Ok(process::getegid()),
        numbers::SETEGID => sys_unimp!("setegid", Ok(0)),
        numbers::GETPGID => process::getpgid(a0),
        numbers::SETPGID => process::setpgid(a0, a1),
        numbers::GETRESUID => process::getresuid(a0.into(), a1.into(), a2.into()).map(|_| 0),
        numbers::SETRESUID => sys_unimp!("setresuid", Err(Errno::ENOSYS)),
        numbers::GETRESGID => process::getresgid(a0.into(), a1.into(), a2.into()).map(|_| 0),
        numbers::SETRESGID => sys_unimp!("setresgid", Err(Errno::ENOSYS)),
        numbers::SETREUID => sys_unimp!("setreuid", Err(Errno::ENOSYS)),
        numbers::SETREGID => sys_unimp!("setregid", Err(Errno::ENOSYS)),
        numbers::UMASK => sys_unimp!("umask", Err(Errno::ENOSYS)),

        // Limits
        numbers::GETRUSAGE => sys_unimp!("getrusage", Err(Errno::ENOSYS)),
        numbers::GETRLIMIT => sys_unimp!("getrlimit", Err(Errno::ENOSYS)),
        numbers::SETRLIMIT => sys_unimp!("setrlimit", Err(Errno::ENOSYS)),

        // Futexes
        numbers::FUTEX_WAIT => system::futex_wait(a0.into(), a1 as i32, a2.into()),
        numbers::FUTEX_WAKE => system::futex_wake(a0.into(), a1 != 0),

        // Time
        numbers::TIMER_CREATE => sys_unimp!("timer_create", Ok(0)),
        numbers::TIMER_SET => sys_unimp!("timer_set", Err(Errno::ENOSYS)),
        numbers::TIMER_DELETE => sys_unimp!("timer_delete", Err(Errno::ENOSYS)),
        numbers::ITIMER_GET => system::itimer_get(a0, a1.into()),
        numbers::ITIMER_SET => system::itimer_set(a0, a1.into(), a2.into()),
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

        // Modules
        numbers::MODULE_INSERT => module::module_insert(a0.into(), a1.into()).map(|_| 0),
        numbers::MODULE_REMOVE => sys_unimp!("module_remove", Err(Errno::ENOSYS)),

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
