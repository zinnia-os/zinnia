mod memory;
mod module;
mod numbers;
mod process;
mod signal;
mod socket;
mod system;
mod vfs;

use crate::{
    arch::sched::Context,
    memory::{UserCStr, UserPtr, VirtAddr},
    posix::errno::{EResult, Errno},
};

pub trait SyscallReturn {
    fn into_ctx(self, ctx: &mut Context);
}

pub trait SyscallArg: Copy + Clone {
    fn from_usize(arg: usize) -> EResult<Self>;
}

impl<T: SyscallReturn> SyscallReturn for EResult<T> {
    fn into_ctx(self, ctx: &mut Context) {
        match self {
            Ok(res) => res.into_ctx(ctx),
            Err(err) => ctx.set_return(0, err as usize),
        }
    }
}

macro_rules! impl_syscall_traits {
    ($($typ:ty),*) => {
        $(
            impl SyscallReturn for $typ {
                fn into_ctx(self, ctx: &mut Context) {
                    ctx.set_return(self as usize, 0);
                }
            }
            impl SyscallArg for $typ {
                fn from_usize(arg: usize) -> EResult<$typ> {
                    Ok(arg as $typ)
                }
            }
        )*
    }
}

impl_syscall_traits!(u8, u16, u32, u64, usize);
impl_syscall_traits!(i8, i16, i32, i64, isize);

impl SyscallReturn for () {
    fn into_ctx(self, ctx: &mut Context) {
        ctx.set_return(0, 0);
    }
}

impl SyscallArg for bool {
    fn from_usize(arg: usize) -> EResult<Self> {
        Ok(arg != 0)
    }
}

impl SyscallArg for VirtAddr {
    fn from_usize(arg: usize) -> EResult<VirtAddr> {
        Ok(VirtAddr::new(arg))
    }
}

impl<T: Copy> SyscallArg for UserPtr<T> {
    fn from_usize(arg: usize) -> EResult<UserPtr<T>> {
        Ok(UserPtr::new(VirtAddr::new(arg)))
    }
}

impl SyscallArg for UserCStr {
    fn from_usize(arg: usize) -> EResult<UserCStr> {
        Ok(UserCStr::new(VirtAddr::new(arg)))
    }
}

#[macro_export]
macro_rules! wrap_syscall {
    attr() {
        $( #[ $($meta:meta)* ] )?
        $vis:vis fn $name:ident (
            $( $arg:ident : $arg_ty:ty ),* $(,)?
        ) -> $ret:ty $body:block
    } => {
        $( #[ $($meta)* ] )?
        $vis fn $name(ctx: &mut $crate::arch::sched::Context) {
            fn inner($($arg : $arg_ty),*) -> $ret $body

            fn inner_wrapper(ctx: &mut $crate::arch::sched::Context) -> $ret {
                let ($($arg),*) = (
                    $(
                        paste::paste! {
                            <$arg_ty as $crate::syscall::SyscallArg>::from_usize(
                                ctx.[< arg ${index(0)} >]()
                            )?
                        }
                    ),*
                );

                #[cfg(feature = "syscall_log")]
                $crate::log!(
                    concat!(stringify!($name), " called with args:", $(" ", stringify!($arg), "={:?}"),*),
                    $($arg),*
                );

                let result = inner($($arg),*);
                #[cfg(feature = "syscall_log")]
                $crate::log!("-> {:?}", result);
                result

            }

            $crate::syscall::SyscallReturn::into_ctx(inner_wrapper(ctx), ctx);
        }
    }
}

macro_rules! sys_unimpl {
    ($name:expr, $ret:expr) => {{
        #[wrap_syscall]
        fn unimp() -> $crate::posix::errno::EResult<usize> {
            $crate::warn!("Call to unimplemented syscall {}", $name);
            $ret
        }
        unimp
    }};
}

/// Executes the syscall as identified by `num`.
/// Returns a tuple of (value, error) to the user. An error code of 0 inidcates success.
/// If the error code is not 0, `value` is not valid and indicates failure.
pub(crate) fn dispatch(frame: &mut Context) {
    let handler: fn(&mut Context) = match frame.syscall_number() {
        // System control
        numbers::SYSLOG => system::syslog,
        numbers::GETUNAME => system::getuname,
        numbers::SETUNAME => system::setuname,
        numbers::ARCHCTL => system::archctl,
        numbers::REBOOT => system::reboot,

        // Mapped memory
        numbers::MMAP => memory::mmap,
        numbers::MUNMAP => memory::munmap,
        numbers::MPROTECT => memory::mprotect,
        numbers::MSYNC => memory::msync,
        numbers::MADVISE => sys_unimpl!("madvise", Err(Errno::ENOSYS)),

        // Signals
        numbers::SIGPROCMASK => signal::sigprocmask,
        numbers::SIGSUSPEND => sys_unimpl!("sigsuspend", Err(Errno::ENOSYS)),
        numbers::SIGPENDING => sys_unimpl!("sigpending", Err(Errno::ENOSYS)),
        numbers::SIGACTION => signal::sigaction,
        numbers::SIGTIMEDWAIT => sys_unimpl!("sigtimedwait", Err(Errno::ENOSYS)),
        numbers::SIGALTSTACK => sys_unimpl!("sigaltstack", Err(Errno::ENOSYS)),
        numbers::SIGRETURN => signal::sigreturn(frame),

        // Processes
        numbers::EXIT => process::exit(frame.arg0()),
        numbers::EXECVE => process::execve,
        numbers::FORK => {
            SyscallReturn::into_ctx(process::fork(frame), frame);
            return;
        }
        numbers::KILL => signal::kill,
        numbers::GETTID => process::gettid,
        numbers::GETPID => process::getpid,
        numbers::GETPPID => process::getppid,
        numbers::WAITID => sys_unimpl!("waitid", Err(Errno::ENOSYS)),
        numbers::WAITPID => process::waitpid,

        // Threads
        numbers::THREAD_CREATE => process::thread_create,
        numbers::THREAD_KILL => process::thread_kill,
        numbers::THREAD_EXIT => process::thread_exit(),
        numbers::THREAD_SETNAME => process::thread_setname,
        numbers::THREAD_GETNAME => process::thread_getname,

        // VFS
        numbers::PREAD => vfs::pread,
        numbers::READV => vfs::readv,
        numbers::PWRITE => vfs::pwrite,
        numbers::WRITEV => vfs::writev,
        numbers::SEEK => vfs::seek,
        numbers::IOCTL => vfs::ioctl,
        numbers::OPENAT => vfs::openat,
        numbers::CLOSE => vfs::close,
        numbers::FSTAT => vfs::fstat,
        numbers::FSTATAT => vfs::fstatat,
        numbers::STATVFS => vfs::statvfs,
        numbers::FSTATVFS => vfs::fstatvfs,
        numbers::FACCESSAT => vfs::faccessat,
        numbers::FCNTL => vfs::fcntl,
        numbers::FTRUNCATE => vfs::ftruncate,
        numbers::FALLOCATE => vfs::fallocate,
        numbers::UTIMENSAT => sys_unimpl!("utimensat", Err(Errno::ENOSYS)),
        numbers::MKNODAT => sys_unimpl!("mknodat", Err(Errno::ENOSYS)),
        numbers::GETCWD => vfs::getcwd,
        numbers::CHDIR => vfs::chdir,
        numbers::FCHDIR => vfs::fchdir,
        numbers::MKDIRAT => vfs::mkdirat,
        numbers::RMDIRAT => sys_unimpl!("rmdirat", Err(Errno::ENOSYS)),
        numbers::GETDENTS => vfs::getdents,
        numbers::RENAMEAT => vfs::renameat,
        numbers::FCHMOD => vfs::fchmod,
        numbers::FCHMODAT => vfs::fchmodat,
        numbers::FCHOWNAT => vfs::fchownat,
        numbers::LINKAT => vfs::linkat,
        numbers::SYMLINKAT => vfs::symlinkat,
        numbers::UNLINKAT => vfs::unlinkat,
        numbers::READLINKAT => vfs::readlinkat,
        numbers::FLOCK => vfs::flock,
        numbers::PPOLL => vfs::ppoll,
        numbers::DUP => vfs::dup,
        numbers::DUP3 => vfs::dup3,
        numbers::SYNC => sys_unimpl!("sync", Err(Errno::ENOSYS)),
        numbers::FSYNC => sys_unimpl!("fsync", Err(Errno::ENOSYS)),
        numbers::FDATASYNC => sys_unimpl!("fdatasync", Err(Errno::ENOSYS)),
        numbers::CHROOT => vfs::chroot,
        numbers::MOUNT => vfs::mount,
        numbers::UMOUNT => vfs::umount,
        numbers::PIPE => vfs::pipe,
        numbers::EPOLL_CREATE => vfs::epoll_create,
        numbers::EPOLL_CTL => vfs::epoll_ctl,
        numbers::EPOLL_PWAIT => vfs::epoll_pwait,
        numbers::TIMERFD_CREATE => vfs::timerfd_create,
        numbers::TIMERFD_GETTIME => vfs::timerfd_gettime,
        numbers::TIMERFD_SETTIME => vfs::timerfd_settime,
        numbers::SIGNALFD_CREATE => vfs::signalfd_create,

        // Sockets
        numbers::SOCKET => socket::socket,
        numbers::SOCKETPAIR => socket::socketpair,
        numbers::SHUTDOWN => socket::shutdown,
        numbers::BIND => socket::bind,
        numbers::CONNECT => socket::connect,
        numbers::ACCEPT => socket::accept,
        numbers::LISTEN => socket::listen,
        numbers::GETPEERNAME => socket::getpeername,
        numbers::GETSOCKNAME => socket::getsockname,
        numbers::GETSOCKOPT => socket::getsockopt,
        numbers::SETSOCKOPT => socket::setsockopt,
        numbers::SENDMSG => socket::sendmsg,
        numbers::RECVMSG => socket::recvmsg,

        // Identity
        numbers::GETGROUPS => sys_unimpl!("getgroups", Ok(0)),
        numbers::SETGROUPS => sys_unimpl!("setgroups", Err(Errno::ENOSYS)),
        numbers::GETSID => process::getsid,
        numbers::SETSID => process::setsid,
        numbers::SETUID => sys_unimpl!("setuid", Ok(0)),
        numbers::GETUID => process::getuid,
        numbers::SETGID => sys_unimpl!("setgid", Ok(0)),
        numbers::GETGID => process::getgid,
        numbers::GETEUID => process::geteuid,
        numbers::SETEUID => sys_unimpl!("seteuid", Ok(0)),
        numbers::GETEGID => process::getegid,
        numbers::SETEGID => sys_unimpl!("setegid", Ok(0)),
        numbers::GETPGID => process::getpgid,
        numbers::SETPGID => process::setpgid,
        numbers::GETRESUID => process::getresuid,
        numbers::SETRESUID => sys_unimpl!("setresuid", Err(Errno::ENOSYS)),
        numbers::GETRESGID => process::getresgid,
        numbers::SETRESGID => sys_unimpl!("setresgid", Err(Errno::ENOSYS)),
        numbers::SETREUID => sys_unimpl!("setreuid", Err(Errno::ENOSYS)),
        numbers::SETREGID => sys_unimpl!("setregid", Err(Errno::ENOSYS)),
        numbers::UMASK => process::umask,

        // Limits
        numbers::GETRUSAGE => sys_unimpl!("getrusage", Err(Errno::ENOSYS)),
        numbers::GETRLIMIT => sys_unimpl!("getrlimit", Err(Errno::ENOSYS)),
        numbers::SETRLIMIT => sys_unimpl!("setrlimit", Err(Errno::ENOSYS)),

        // Futexes
        numbers::FUTEX_WAIT => system::futex_wait,
        numbers::FUTEX_WAKE => system::futex_wake,

        // Time
        numbers::TIMER_CREATE => sys_unimpl!("timer_create", Ok(0)),
        numbers::TIMER_SET => sys_unimpl!("timer_set", Err(Errno::ENOSYS)),
        numbers::TIMER_DELETE => sys_unimpl!("timer_delete", Err(Errno::ENOSYS)),
        numbers::ITIMER_GET => system::itimer_get,
        numbers::ITIMER_SET => system::itimer_set,
        numbers::CLOCK_GET => system::clock_get,
        numbers::CLOCK_GETRES => system::clock_getres,

        // Scheduling
        numbers::SLEEP => system::sleep,
        numbers::YIELD => sys_unimpl!("yield", Ok(0)),
        numbers::GETPRIORITY => sys_unimpl!("getpriority", Err(Errno::ENOSYS)),
        numbers::SETPRIORITY => sys_unimpl!("setpriority", Err(Errno::ENOSYS)),
        numbers::SCHED_GETPARAM => sys_unimpl!("sched_getparam", Err(Errno::ENOSYS)),
        numbers::SCHED_SETPARAM => sys_unimpl!("sched_setparam", Err(Errno::ENOSYS)),
        numbers::GETENTROPY => sys_unimpl!("getentropy", Ok(0)),

        // Modules
        numbers::MODULE_INSERT => module::module_insert,
        numbers::MODULE_REMOVE => sys_unimpl!("module_remove", Err(Errno::ENOSYS)),

        num => {
            warn!("Unknown syscall {num}");
            frame.set_return(0, Errno::ENOSYS as usize);
            return;
        }
    };

    handler(frame);
}
