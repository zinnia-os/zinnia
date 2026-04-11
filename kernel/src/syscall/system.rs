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
    util::{event::Event, mutex::spin::SpinMutex},
};
use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FutexKey {
    address_space: usize,
    addr: VirtAddr,
}

#[derive(Debug)]
struct FutexQueue {
    key: FutexKey,
    event: Event,
}

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

pub fn futex_wait(pointer: VirtAddr, expected: i32, timeout: VirtAddr) -> EResult<usize> {
    let pointer = UserPtr::<i32>::new(pointer);
    let deadline = read_timeout_deadline(timeout)?;
    let timeout_guard = deadline.map(clock::timeout_at);
    let queue = get_futex_queue(pointer.addr());
    let waiter = queue.event.guard();

    if pointer.read().ok_or(Errno::EFAULT)? != expected {
        return Err(Errno::EAGAIN);
    }

    if timeout_guard.as_ref().is_some_and(|guard| guard.expired()) {
        return Err(Errno::ETIMEDOUT);
    }

    waiter.wait();

    if Scheduler::get_current().has_pending_signals() {
        return Err(Errno::EINTR);
    }

    if timeout_guard.as_ref().is_some_and(|guard| guard.expired()) {
        return Err(Errno::ETIMEDOUT);
    }

    Ok(0)
}

pub fn futex_wake(pointer: VirtAddr, all: bool) -> EResult<usize> {
    let queue = get_futex_queue(pointer);

    Ok(if all {
        queue.event.wake_all()
    } else {
        queue.event.wake_one()
    })
}

pub fn itimer_get(which: usize, curr_value: VirtAddr) -> EResult<usize> {
    if which != ITIMER_REAL {
        return Err(Errno::EINVAL);
    }

    let proc = Scheduler::get_current().get_process();
    let current = proc.get_real_timer(clock::get_elapsed());
    let mut curr_value = UserPtr::<itimerval>::new(curr_value);
    curr_value.write(current).ok_or(Errno::EFAULT)?;

    Ok(0)
}

pub fn itimer_set(which: usize, new_value: VirtAddr, old_value: VirtAddr) -> EResult<usize> {
    if which != ITIMER_REAL {
        return Err(Errno::EINVAL);
    }

    let new_value = UserPtr::<itimerval>::new(new_value)
        .read()
        .ok_or(Errno::EFAULT)?;
    let proc = Scheduler::get_current().get_process();
    let old = proc.set_real_timer(clock::get_elapsed(), new_value)?;

    if !old_value.is_null() {
        let mut old_value = UserPtr::<itimerval>::new(old_value);
        old_value.write(old).ok_or(Errno::EFAULT)?;
    }

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

fn get_futex_queue(pointer: VirtAddr) -> Arc<FutexQueue> {
    let proc = Scheduler::get_current().get_process();
    let key = FutexKey {
        address_space: Arc::as_ptr(&proc.address_space) as usize,
        addr: pointer,
    };

    let mut futexes = FUTEXES.lock();
    if let Some(queue) = futexes.iter().find(|queue| queue.key == key) {
        return queue.clone();
    }

    let queue = Arc::new(FutexQueue {
        key,
        event: Event::new(),
    });
    futexes.push(queue.clone());
    queue
}

fn read_timeout_deadline(timeout: VirtAddr) -> EResult<Option<usize>> {
    if timeout.is_null() {
        return Ok(None);
    }

    let timeout = UserPtr::<timespec>::new(timeout)
        .read()
        .ok_or(Errno::EFAULT)?;
    let duration = timespec_to_ns(timeout)?;
    let deadline = clock::get_elapsed()
        .checked_add(duration)
        .ok_or(Errno::EINVAL)?;

    Ok(Some(deadline))
}

fn timespec_to_ns(value: timespec) -> EResult<usize> {
    if value.tv_sec < 0 || value.tv_nsec < 0 || value.tv_nsec >= 1_000_000_000 {
        return Err(Errno::EINVAL);
    }

    let seconds = (value.tv_sec as usize)
        .checked_mul(1_000_000_000)
        .ok_or(Errno::EINVAL)?;

    seconds
        .checked_add(value.tv_nsec as usize)
        .ok_or(Errno::EINVAL)
}

static FUTEXES: SpinMutex<Vec<Arc<FutexQueue>>> = SpinMutex::new(Vec::new());
