use crate::{
    posix::errno::{EResult, Errno},
    process::{PROCESS_TABLE, signal},
    uapi,
};
use alloc::sync::Weak;
use core::ops::Bound;
use core::time::Duration;

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
        self.interval = value.it_interval.to_duration()?;

        let initial = value.it_value.to_duration()?;
        self.next_deadline = if initial.is_zero() {
            None
        } else {
            Some(now.checked_add(initial).ok_or(Errno::EINVAL)?)
        };

        Ok(old)
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

fn poll_process_timer(now: Duration, proc: &alloc::sync::Arc<crate::process::Process>) {
    let should_signal = {
        let mut timer = proc.real_timer.lock();
        match timer.next_deadline {
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
        }
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
