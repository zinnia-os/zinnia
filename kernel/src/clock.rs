//! Global timer management.
// TODO: Try to get rid of some locks.

use super::util::mutex::spin::SpinMutex;
use crate::{
    process::{self, task::Task},
    sched::Scheduler,
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use core::time::Duration;

#[initgraph::task(name = "generic.clock")]
pub fn CLOCK_STAGE() {}

pub trait ClockSource: Send {
    fn name(&self) -> &'static str;

    /// A priority of a clock source. A high value equals a high priority.
    fn get_priority(&self) -> u8;

    /// Resets the elapsed time to start counting from now.
    fn reset(&mut self);

    /// Gets the elapsed time since initialization of this timer.
    fn elapsed(&self) -> Duration;
}

#[derive(Debug, PartialEq)]
pub enum ClockError {
    /// The clock source has a lesser priority.
    LowerPriority,
    /// The clock source is unavailable.
    Unavailable,
    /// The clock source is not sane.
    InvalidConfiguration,
    /// The clock source could not be calibrated.
    UnableToSetup,
}

/// Gets the elapsed time since initialization of this timer.
#[track_caller]
pub fn get_elapsed() -> Duration {
    let guard = CLOCK.lock();
    match &guard.current {
        Some(x) => x.elapsed() + guard.counter_base,
        None => Duration::ZERO,
    }
}

/// Switches to a new clock source if it is of higher priority.
pub fn switch(mut new_source: Box<dyn ClockSource>) -> Result<(), ClockError> {
    let name = new_source.name();

    let old_source = {
        let mut clock = CLOCK.lock();
        if let Some(current) = &clock.current
            && new_source.get_priority() <= current.get_priority()
        {
            return Err(ClockError::LowerPriority);
        }

        // Save the current counter without recursively taking CLOCK.
        let elapsed = match &clock.current {
            Some(current) => current.elapsed() + clock.counter_base,
            None => Duration::ZERO,
        };

        clock.counter_base = elapsed;
        new_source.reset();
        clock.current.replace(new_source)
    };

    drop(old_source);
    log!("Switching to clock source \"{}\"", name);
    return Ok(());
}

pub fn has_clock() -> bool {
    return CLOCK.lock().current.is_some();
}

struct TimeoutWaiter {
    task: Arc<Task>,
    deadline: Duration,
    active: AtomicBool,
    fired: AtomicBool,
}

pub struct TimeoutGuard {
    waiter: Arc<TimeoutWaiter>,
}

impl TimeoutGuard {
    pub fn expired(&self) -> bool {
        self.waiter.fired.load(Ordering::Acquire) || get_elapsed() >= self.waiter.deadline
    }
}

impl Drop for TimeoutGuard {
    fn drop(&mut self) {
        self.waiter.active.store(false, Ordering::Release);
    }
}

pub fn timeout_at(deadline: Duration) -> TimeoutGuard {
    let waiter = Arc::new(TimeoutWaiter {
        task: Scheduler::get_current(),
        deadline,
        active: AtomicBool::new(true),
        fired: AtomicBool::new(false),
    });

    TIMEOUT_WAITERS.lock().push(waiter.clone());
    TimeoutGuard { waiter }
}

pub fn handle_tick() {
    let now = get_elapsed();
    wake_timeout_waiters(now);
    process::itimer::poll_interval_timers(now);
    crate::vfs::timerfd::poll_timerfds(now);
}

/// Sleeps the current task for at least `duration`, yielding the CPU while waiting.
pub fn sleep(duration: Duration) {
    let deadline = get_elapsed().saturating_add(duration);
    let guard = timeout_at(deadline);
    while !guard.expired() {
        crate::percpu::CpuData::get().scheduler.do_yield();
    }
}

/// Waits for at least `duration`.
pub fn block(duration: Duration) -> Result<(), ClockError> {
    if CLOCK.lock().current.is_none() {
        error!(
            "Unable to sleep for {duration:?}. No clock source available, this would block forever!"
        );
        return Err(ClockError::Unavailable);
    }

    let target = get_elapsed() + duration;
    while get_elapsed() < target {}
    return Ok(());
}

fn wake_timeout_waiters(now: Duration) {
    let mut expired = Vec::new();
    let mut removed = Vec::new();

    {
        let mut waiters = TIMEOUT_WAITERS.lock();
        let mut i = 0;
        while i < waiters.len() {
            let waiter = &waiters[i];
            let remove = if !waiter.active.load(Ordering::Acquire) {
                true
            } else if waiter.deadline <= now
                && waiter
                    .active
                    .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                waiter.fired.store(true, Ordering::Release);
                expired.push(waiter.task.clone());
                true
            } else {
                false
            };

            if remove {
                removed.push(waiters.swap_remove(i));
            } else {
                i += 1;
            }
        }
    }

    drop(removed);
    for task in expired {
        Scheduler::wake_task(task);
    }
}

struct Clock {
    /// The active clock source.
    current: Option<Box<dyn ClockSource>>,
    /// An offset to add to the read counter.
    counter_base: Duration,
}

static CLOCK: SpinMutex<Clock> = SpinMutex::new(Clock {
    current: None,
    counter_base: Duration::ZERO,
});

static TIMEOUT_WAITERS: SpinMutex<Vec<Arc<TimeoutWaiter>>> = SpinMutex::new(Vec::new());

/// Offset (in nanoseconds) from monotonic boot time to the Unix epoch, or
/// [`i64::MIN`] when the wall clock has not been set.
static BOOT_REALTIME_NS: AtomicI64 = AtomicI64::new(i64::MIN);

pub fn set_realtime(now_unix: Duration) {
    let elapsed = get_elapsed().as_nanos() as i64;
    let base = (now_unix.as_nanos() as i64).saturating_sub(elapsed);
    BOOT_REALTIME_NS.store(base, Ordering::Release);
}
pub fn realtime() -> Option<Duration> {
    let base = BOOT_REALTIME_NS.load(Ordering::Acquire);
    if base == i64::MIN {
        return None;
    }
    let ns = base.saturating_add(get_elapsed().as_nanos() as i64);
    Some(Duration::from_nanos(ns.max(0) as u64))
}

/// Polls `f` until it returns `true` or the timeout elapses.
pub fn poll_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let deadline = get_elapsed().saturating_add(timeout);
    loop {
        if f() {
            return true;
        }
        if get_elapsed() >= deadline {
            return false;
        }
        let _ = block(Duration::from_millis(10));
    }
}
