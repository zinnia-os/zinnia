//! Global timer management.
// TODO: Try to get rid of some locks.

use super::util::mutex::spin::SpinMutex;
use crate::{
    process::{self, task::Task},
    sched::Scheduler,
};
use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use core::time::Duration;
use core::{
    mem,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, AtomicUsize},
};
use intrusive_collections::{LinkedList, LinkedListAtomicLink, UnsafeRef, intrusive_adapter};

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
    link: LinkedListAtomicLink,
    defer_link: LinkedListAtomicLink,
}

intrusive_adapter!(TimeoutLink = UnsafeRef<TimeoutWaiter>: TimeoutWaiter { link => LinkedListAtomicLink });
intrusive_adapter!(TimeoutDeferLink = UnsafeRef<TimeoutWaiter>: TimeoutWaiter { defer_link => LinkedListAtomicLink });

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
        link: LinkedListAtomicLink::new(),
        defer_link: LinkedListAtomicLink::new(),
    });

    TIMEOUT_WAITERS
        .lock()
        .push_back(unsafe { UnsafeRef::from_raw(Arc::into_raw(waiter.clone())) });
    TimeoutGuard { waiter }
}

/// Runs from the timer IRQ: must not allocate or drop the last reference to a
/// task. Heavy work is deferred to [`ktimer_fn`].
pub fn handle_tick() {
    let now = get_elapsed();
    wake_timeout_waiters(now);

    if DEFERRED_COUNT.load(Ordering::Relaxed) > 0
        || crate::vfs::timerfd::active_count() > 0
        || process::itimer::armed_count() > 0
    {
        wake_ktimer();
    }
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
    let mut removed: LinkedList<TimeoutDeferLink> = LinkedList::new(TimeoutDeferLink::NEW);

    {
        let mut waiters = TIMEOUT_WAITERS.lock();
        let mut cursor = waiters.front_mut();
        loop {
            let Some(waiter) = cursor.get() else {
                break;
            };

            let remove = if !waiter.active.load(Ordering::Acquire) {
                true
            } else if waiter.deadline <= now
                && waiter
                    .active
                    .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                waiter.fired.store(true, Ordering::Release);
                Scheduler::wake_task(waiter.task.clone());
                true
            } else {
                false
            };

            if remove {
                removed.push_back(cursor.remove().unwrap());
            } else {
                cursor.move_next();
            }
        }
    }

    let mut count = 0;
    if !removed.is_empty() {
        let mut drops = DEFERRED_DROPS.lock();
        while let Some(node) = removed.pop_front() {
            drops.push_back(node);
            count += 1;
        }
    }
    if count > 0 {
        DEFERRED_COUNT.fetch_add(count, Ordering::Relaxed);
    }
}

/// Kernel thread that does timer work.
pub extern "C" fn ktimer_fn(_: usize, _: usize) {
    loop {
        loop {
            let node = DEFERRED_DROPS.lock().pop_front();
            let Some(node) = node else {
                break;
            };
            DEFERRED_COUNT.fetch_sub(1, Ordering::Relaxed);
            drop(unsafe { Arc::from_raw(UnsafeRef::into_raw(node)) });
        }

        let now = get_elapsed();
        crate::vfs::timerfd::poll_timerfds(now);
        process::itimer::poll_interval_timers(now);

        crate::percpu::CpuData::get().scheduler.do_yield();
    }
}

pub fn set_ktimer_task(task: Arc<Task>) {
    let old = KTIMER_TASK.swap(Arc::into_raw(task) as *mut _, Ordering::AcqRel);
    debug_assert!(old.is_null());
}

fn wake_ktimer() {
    let ptr = KTIMER_TASK.load(Ordering::Acquire);
    if ptr.is_null() {
        // Too early in boot; work is picked up once the thread exists.
        return;
    }

    let task = unsafe { Arc::from_raw(ptr) };
    let clone = task.clone();
    mem::forget(task);
    Scheduler::wake_task(clone);
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

// Lock order: TIMEOUT_WAITERS before DEFERRED_DROPS (never the reverse).
static TIMEOUT_WAITERS: SpinMutex<LinkedList<TimeoutLink>> =
    SpinMutex::new(LinkedList::new(TimeoutLink::NEW));

/// Waiters removed in IRQ context, awaiting their final drop in ktimer.
static DEFERRED_DROPS: SpinMutex<LinkedList<TimeoutDeferLink>> =
    SpinMutex::new(LinkedList::new(TimeoutDeferLink::NEW));

static DEFERRED_COUNT: AtomicUsize = AtomicUsize::new(0);

static KTIMER_TASK: AtomicPtr<Task> = AtomicPtr::new(null_mut());

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
