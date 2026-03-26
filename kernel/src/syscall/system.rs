use crate::{
    clock,
    irq::lock::IrqLock,
    memory::{VirtAddr, user::UserPtr},
    posix::{
        errno::{EResult, Errno},
        utsname::UTSNAME,
    },
    sched::Scheduler,
    uapi::{self, reboot::*, time::*},
};
use alloc::string::String;
use core::fmt::Write;

pub fn archctl(cmd: usize, arg: usize) -> EResult<usize> {
    crate::arch::cpu::archctl(cmd, arg)
}

pub fn getuname(addr: VirtAddr) -> EResult<usize> {
    let mut addr = UserPtr::new(addr);
    addr.write(*UTSNAME.lock()).ok_or(Errno::EINVAL)?;

    Ok(0)
}

pub fn setuname(addr: VirtAddr) -> EResult<usize> {
    let addr = UserPtr::new(addr);

    let proc = Scheduler::get_current().get_process();
    // Only allow the superuser to change the uname.
    if proc.identity.lock().user_id != 0 {
        return Err(Errno::EPERM);
    }

    let mut utsname = UTSNAME.lock();
    *utsname = addr.read().ok_or(Errno::EINVAL)?;

    Ok(0)
}

pub fn clock_get(clockid: uapi::clockid_t, tp: VirtAddr) -> EResult<usize> {
    let _ = clockid; // TODO: Respect clockid

    let mut tp = UserPtr::new(tp);

    let elapsed = clock::get_elapsed();
    const NS_TO_SEC: usize = 1000 * 1000 * 1000;

    tp.write(timespec {
        tv_sec: (elapsed / NS_TO_SEC) as _,
        tv_nsec: (elapsed % NS_TO_SEC) as _,
    })
    .ok_or(Errno::EINVAL)?;

    Ok(0)
}

const LOG_EMERG: usize = 0;
const LOG_ALERT: usize = 1;
const LOG_CRIT: usize = 2;
const LOG_ERR: usize = 3;
const LOG_WARNING: usize = 4;
const LOG_NOTICE: usize = 5;
const LOG_INFO: usize = 6;
const LOG_DEBUG: usize = 7;

pub fn syslog(level: usize, ptr: VirtAddr, len: usize) -> EResult<usize> {
    let ptr = UserPtr::<u8>::new(ptr);
    if ptr.is_null() {
        return Ok(0);
    }

    let mut buf = vec![0u8; len];
    ptr.read_slice(&mut buf).ok_or(Errno::EINVAL)?;

    let current_time = clock::get_elapsed();
    let _lock = IrqLock::lock();
    let mut writer = crate::log::GLOBAL_LOGGERS.lock();
    _ = writer.write_fmt(format_args!(
        "[{:5}.{:06}] \x1b[0m",
        current_time / 1_000_000_000,
        (current_time / 1000) % 1_000_000,
    ));
    _ = writer.write_fmt(format_args!(
        "[{}] {}",
        match level {
            LOG_EMERG => "EMERG",
            LOG_ALERT => "ALERT",
            LOG_CRIT => "CRIT",
            LOG_ERR => "ERR",
            LOG_WARNING => "WARNING",
            LOG_NOTICE => "NOTICE",
            LOG_INFO => "INFO",
            LOG_DEBUG => "DEBUG",
            _ => "?",
        },
        String::from_utf8_lossy(&buf)
    ));
    _ = writer.write_fmt(format_args!("\x1b[0m\n"));

    Ok(0)
}

pub fn reboot(magic: u32, cmd: u32) -> EResult<usize> {
    if magic != 0xdeadbeef {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let identity = proc.identity.lock();
    if identity.user_id != 0 {
        return Err(Errno::EPERM);
    }

    match cmd {
        RB_DISABLE_CAD => {
            warn!("RB_DISABLE_CAD is unimplemented");
        }
        RB_ENABLE_CAD => {
            warn!("RB_ENABLE_CAD is unimplemented");
        }
        RB_POWER_OFF => {
            todo!("Power off");
        }
        _ => {
            warn!("Unknown reboot command {:#x}", cmd);
            return Err(Errno::EINVAL);
        }
    }
    Ok(0)
}

pub fn sleep(request: VirtAddr, remainder: VirtAddr) -> EResult<usize> {
    let request = UserPtr::new(request);
    // TODO
    let _remainder = UserPtr::<timespec>::new(remainder);

    let ts: timespec = request.read().ok_or(Errno::EFAULT)?;

    // TODO: Actually sleep instead of blocking.
    clock::block_ns(ts.tv_nsec as usize).unwrap();

    Ok(0)
}
