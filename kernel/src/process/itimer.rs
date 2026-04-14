use alloc::{sync::Weak, vec::Vec};

use crate::{
    posix::errno::{EResult, Errno},
    process::{PROCESS_TABLE, signal},
    uapi,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct IntervalTimerState {
    interval_ns: usize,
    next_deadline_ns: Option<usize>,
}

impl IntervalTimerState {
    pub(super) fn snapshot(&self, now: usize) -> uapi::time::itimerval {
        uapi::time::itimerval {
            it_interval: ns_to_timeval(self.interval_ns),
            it_value: ns_to_timeval(
                self.next_deadline_ns
                    .map(|deadline| deadline.saturating_sub(now))
                    .unwrap_or(0),
            ),
        }
    }

    pub(super) fn replace(
        &mut self,
        now: usize,
        value: uapi::time::itimerval,
    ) -> EResult<uapi::time::itimerval> {
        let old = self.snapshot(now);
        self.interval_ns = timeval_to_ns(value.it_interval)?;

        let initial_ns = timeval_to_ns(value.it_value)?;
        self.next_deadline_ns = if initial_ns == 0 {
            None
        } else {
            Some(now.checked_add(initial_ns).ok_or(Errno::EINVAL)?)
        };

        Ok(old)
    }
}

pub fn poll_interval_timers(now: usize) {
    let processes: Vec<_> = {
        let table = PROCESS_TABLE.lock();
        table.values().filter_map(Weak::upgrade).collect()
    };

    for proc in processes {
        let should_signal = {
            let mut timer = proc.real_timer.lock();
            match timer.next_deadline_ns {
                Some(deadline) if deadline <= now => {
                    if timer.interval_ns == 0 {
                        timer.next_deadline_ns = None;
                    } else {
                        let mut next_deadline = deadline;
                        loop {
                            let Some(candidate) = next_deadline.checked_add(timer.interval_ns)
                            else {
                                timer.next_deadline_ns = None;
                                break;
                            };

                            if candidate > now {
                                timer.next_deadline_ns = Some(candidate);
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
            continue;
        }

        let thread = {
            let threads = proc.threads.lock();
            threads.first().cloned()
        };

        if let Some(thread) = thread {
            signal::send_signal_to_thread(&thread, signal::Signal::SigAlrm);
        }
    }
}

fn timeval_to_ns(value: uapi::time::timeval) -> EResult<usize> {
    if value.tv_sec < 0 || value.tv_usec < 0 || value.tv_usec >= 1_000_000 {
        return Err(Errno::EINVAL);
    }

    let seconds = (value.tv_sec as usize)
        .checked_mul(1_000_000_000)
        .ok_or(Errno::EINVAL)?;
    let micros = (value.tv_usec as usize)
        .checked_mul(1_000)
        .ok_or(Errno::EINVAL)?;

    seconds.checked_add(micros).ok_or(Errno::EINVAL)
}

const fn ns_to_timeval(value: usize) -> uapi::time::timeval {
    uapi::time::timeval {
        tv_sec: (value / 1_000_000_000) as _,
        tv_usec: ((value % 1_000_000_000) / 1_000) as _,
    }
}
