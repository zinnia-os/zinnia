use crate::{
    posix::errno::{EResult, Errno},
    process::{PROCESS_TABLE, Process, signal},
    uapi,
};
use alloc::sync::Weak;
use core::ops::Bound;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;

/// Amount of currently armed interval timers.
static ARMED_TIMERS: AtomicUsize = AtomicUsize::new(0);

pub fn armed_count() -> usize {
    ARMED_TIMERS.load(Ordering::Relaxed)
}

fn note_transition(was_armed: bool, now_armed: bool) {
    match (was_armed, now_armed) {
        (false, true) => {
            ARMED_TIMERS.fetch_add(1, Ordering::Relaxed);
        }
        (true, false) => {
            ARMED_TIMERS.fetch_sub(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IntervalTimerState {
    interval: Duration,
    next_deadline: Option<Duration>,
}

impl IntervalTimerState {
    pub(super) fn snapshot(&self, now: Duration) -> uapi::time::itimerval {
        uapi::time::itimerval {
            it_interval: uapi::time::timeval::from_duration(self.interval),
            it_value: uapi::time::timeval::from_duration(
                self.next_deadline
                    .map(|deadline| deadline.saturating_sub(now))
                    .unwrap_or(Duration::ZERO),
            ),
        }
    }

    pub(super) fn replace(
        &mut self,
        now: Duration,
        value: uapi::time::itimerval,
    ) -> EResult<uapi::time::itimerval> {
        let old = self.snapshot(now);

        let was_armed = self.next_deadline.is_some();
        self.interval = value.it_interval.to_duration()?;

        let initial = value.it_value.to_duration()?;
        self.next_deadline = if initial.is_zero() {
            None
        } else {
            Some(now.checked_add(initial).ok_or(Errno::EINVAL)?)
        };

        note_transition(was_armed, self.next_deadline.is_some());
        Ok(old)
    }

    /// Disarms the timer.
    pub(crate) fn disarm(&mut self) {
        note_transition(self.next_deadline.is_some(), false);
        self.next_deadline = None;
        self.interval = Duration::ZERO;
    }
}

pub fn poll_interval_timers(now: Duration) {
    let mut last_pid = None;

    loop {
        let Some((pid, proc)) = ({
            let table = PROCESS_TABLE.lock();
            let mut iter = match last_pid {
                Some(pid) => table.range((Bound::Excluded(pid), Bound::Unbounded)),
                None => table.range(..),
            };

            iter.find_map(|(&pid, proc)| Weak::upgrade(proc).map(|proc| (pid, proc)))
        }) else {
            break;
        };
        last_pid = Some(pid);

        poll_process_timer(now, &proc);
    }
}

fn poll_process_timer(now: Duration, proc: &Process) {
    let should_signal = {
        let mut timer = proc.real_timer.lock();

        let was_armed = timer.next_deadline.is_some();
        let result = match timer.next_deadline {
            Some(deadline) if deadline <= now => {
                if timer.interval.is_zero() {
                    timer.next_deadline = None;
                } else {
                    let mut next_deadline = deadline;
                    loop {
                        let Some(candidate) = next_deadline.checked_add(timer.interval) else {
                            timer.next_deadline = None;
                            break;
                        };

                        if candidate > now {
                            timer.next_deadline = Some(candidate);
                            break;
                        }

                        next_deadline = candidate;
                    }
                }

                true
            }
            Some(_) | None => false,
        };

        note_transition(was_armed, timer.next_deadline.is_some());
        result
    };

    if !should_signal {
        return;
    }

    let thread = {
        let threads = proc.threads.lock();
        threads.first().cloned()
    };

    if let Some(thread) = thread {
        signal::send_signal_to_thread(&thread, signal::Signal::SigAlrm);
    }
}
