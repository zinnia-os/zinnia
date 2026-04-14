//! Global timer management.
// TODO: Try to get rid of some locks.

use super::util::mutex::spin::SpinMutex;
use crate::{process, process::task::Task, sched::Scheduler};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

#[initgraph::task(name = "generic.clock")]
pub fn CLOCK_STAGE() {}

pub trait ClockSource: Send {
    fn name(&self) -> &'static str;

    /// A priority of a clock source. A high value equals a high priority.
    fn get_priority(&self) -> u8;

    /// Sets the elapsed nanoseconds to start counting at.
    fn reset(&mut self);

    /// Gets the elapsed nanoseconds since initialization of this timer.
    fn get_elapsed_ns(&self) -> usize;
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

/// Gets the elapsed nanoseconds since initialization of this timer.
#[track_caller]
pub fn get_elapsed() -> usize {
    let guard = CLOCK.lock();
    match &guard.current {
        Some(x) => x.get_elapsed_ns() + guard.counter_base,
        None => 0,
    }
}

/// Switches to a new clock source if it is of higher priority.
pub fn switch(mut new_source: Box<dyn ClockSource>) -> Result<(), ClockError> {
    // Determine if we should make the switch.
    if let Some(x) = &CLOCK.lock().current {
        let prio = x.get_priority();
        if new_source.get_priority() > prio {
            Ok(())
        } else {
            Err(ClockError::LowerPriority)
        }
    } else {
        Ok(())
    }?;

    log!("Switching to clock source \"{}\"", new_source.name());

    // Save the current counter.
    let elapsed = get_elapsed();
    let mut clock = CLOCK.lock();
    clock.counter_base = elapsed;

    new_source.reset();
    clock.current = Some(new_source);
    return Ok(());
}

pub fn has_clock() -> bool {
    return CLOCK.lock().current.is_some();
}

#[derive(Debug)]
struct TimeoutWaiter {
    task: Arc<Task>,
    deadline: usize,
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

pub fn timeout_at(deadline: usize) -> TimeoutGuard {
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
}

/// Blocking wait for a given amount of nanoseconds.
pub fn block_ns(time: usize) -> Result<(), ClockError> {
    if CLOCK.lock().current.is_none() {
        error!(
            "Unable to sleep for {} nanoseconds. No clock source available, this would block forever!",
            time
        );
        return Err(ClockError::Unavailable);
    }

    let target = get_elapsed() + time;
    while get_elapsed() < target {}
    return Ok(());
}

fn wake_timeout_waiters(now: usize) {
    let mut expired = Vec::new();

    {
        let mut waiters = TIMEOUT_WAITERS.lock();
        waiters.retain(|waiter| {
            if !waiter.active.load(Ordering::Acquire) {
                return false;
            }

            if waiter.deadline <= now
                && waiter
                    .active
                    .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                waiter.fired.store(true, Ordering::Release);
                expired.push(waiter.task.clone());
                return false;
            }

            true
        });
    }

    for task in expired {
        crate::percpu::CpuData::get().scheduler.add_task(task);
    }
}

struct Clock {
    /// The active clock source.
    current: Option<Box<dyn ClockSource>>,
    /// An offset to add to the read counter.
    counter_base: usize,
}

static CLOCK: SpinMutex<Clock> = SpinMutex::new(Clock {
    current: None,
    counter_base: 0,
});

static TIMEOUT_WAITERS: SpinMutex<Vec<Arc<TimeoutWaiter>>> = SpinMutex::new(Vec::new());
