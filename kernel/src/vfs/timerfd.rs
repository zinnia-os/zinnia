use crate::{
    clock,
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::time::{itimerspec, timespec},
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::{
        File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
    },
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};

/// Maximum number of nanoseconds in a [`timespec`] field, used for validation.
const NS_PER_SEC: usize = 1_000_000_000;

#[derive(Default)]
struct TimerfdState {
    /// Reload interval in nanoseconds. Zero indicates a one-shot timer.
    interval_ns: usize,
    /// Absolute monotonic deadline (nanoseconds since boot) of the next
    /// expiration, or `None` when the timer is disarmed.
    next_deadline_ns: Option<usize>,
    /// Number of expirations that have not yet been read by userspace.
    expirations: u64,
}

pub struct TimerfdFile {
    state: SpinMutex<TimerfdState>,
    event: Event,
}

impl TimerfdFile {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: SpinMutex::new(TimerfdState::default()),
            event: Event::new(),
        })
    }

    /// Read the current `itimerspec` view of the timer.
    fn snapshot(&self, now: usize) -> itimerspec {
        let state = self.state.lock();
        itimerspec {
            it_interval: ns_to_timespec(state.interval_ns),
            it_value: ns_to_timespec(
                state
                    .next_deadline_ns
                    .map(|deadline| deadline.saturating_sub(now))
                    .unwrap_or(0),
            ),
        }
    }

    /// Configure the timer. `initial_ns` is the absolute deadline (since
    /// boot) of the first expiration. `Some(0)` disarms the timer.
    /// Returns the previous itimerspec.
    pub fn settime(
        self: &Arc<Self>,
        now: usize,
        initial_deadline_ns: Option<usize>,
        interval_ns: usize,
    ) -> itimerspec {
        let old = self.snapshot(now);
        {
            let mut state = self.state.lock();
            state.interval_ns = interval_ns;
            state.next_deadline_ns = initial_deadline_ns;
            // Setting the timer (whether arming or disarming) clears any
            // pending expirations to mirror Linux semantics.
            state.expirations = 0;
        }
        // (Re)register on the global active list whenever the timer is
        // armed. Disarming leaves the entry; the tick handler will simply
        // see `next_deadline_ns == None`.
        if initial_deadline_ns.is_some() {
            register(self);
        }
        // Wake any pollers; readers waiting on a previously-armed timer
        // should observe a possibly-changed (or now empty) state.
        self.event.wake_all();
        old
    }

    pub fn gettime(&self, now: usize) -> itimerspec {
        self.snapshot(now)
    }

    /// Called from the timer tick to advance the timer. Returns true if any
    /// expiration occurred (in which case waiters were woken).
    fn advance(&self, now: usize) -> bool {
        {
            let mut state = self.state.lock();
            let Some(deadline) = state.next_deadline_ns else {
                return false;
            };
            if now < deadline {
                return false;
            }

            // Compute the number of expirations since `deadline` and the
            // next deadline (if periodic).
            if state.interval_ns == 0 {
                state.expirations = state.expirations.saturating_add(1);
                state.next_deadline_ns = None;
            } else {
                let elapsed = now - deadline;
                let extra = (elapsed / state.interval_ns) as u64;
                let count = 1u64.saturating_add(extra);
                state.expirations = state.expirations.saturating_add(count);
                let next = deadline + state.interval_ns * (count as usize);
                state.next_deadline_ns = Some(next);
            }
        }
        self.event.wake_all();
        true
    }

    fn expirations(&self) -> u64 {
        self.state.lock().expirations
    }
}

impl FileOps for TimerfdFile {
    fn read(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        if buf.len() < 8 {
            return Err(Errno::EINVAL);
        }

        loop {
            let guard = self.event.guard();

            {
                // Update the count if the deadline already passed but the
                // tick hasn't yet processed it.
                let now = clock::get_elapsed();
                self.advance(now);
            }

            let count = {
                let mut state = self.state.lock();
                let count = state.expirations;
                if count != 0 {
                    state.expirations = 0;
                }
                count
            };

            if count != 0 {
                let bytes = count.to_ne_bytes();
                let n = buf.copy_from_slice(&bytes)?;
                if (n as usize) < bytes.len() {
                    return Err(Errno::EFAULT);
                }
                return Ok(n);
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            }

            guard.wait();

            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn write(&self, _file: &File, _buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        Err(Errno::EINVAL)
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let now = clock::get_elapsed();
        self.advance(now);
        let mut revents = PollFlags::empty();
        if self.expirations() != 0 {
            revents |= PollFlags::In;
        }
        Ok(revents & mask)
    }

    fn poll_events(&self, _file: &File, _mask: PollFlags) -> PollEventSet<'_> {
        PollEventSet::one(&self.event)
    }
}

static ACTIVE_TIMERS: SpinMutex<Vec<Weak<TimerfdFile>>> = SpinMutex::new(Vec::new());

fn register(timer: &Arc<TimerfdFile>) {
    let mut list = ACTIVE_TIMERS.lock();
    let weak = Arc::downgrade(timer);
    // Avoid duplicate registrations for the same timer.
    if list
        .iter()
        .any(|existing| existing.as_ptr() == weak.as_ptr())
    {
        return;
    }
    list.push(weak);
}

/// Called from the periodic clock tick. Advances all registered timerfds and
/// wakes any waiters whose deadlines have elapsed. Drops registrations whose
/// owning files have been closed.
pub fn poll_timerfds(now: usize) {
    let snapshot: Vec<Arc<TimerfdFile>> = {
        let mut list = ACTIVE_TIMERS.lock();
        list.retain(|w| w.strong_count() > 0);
        list.iter().filter_map(Weak::upgrade).collect()
    };

    for timer in snapshot {
        timer.advance(now);
    }
}

pub fn timespec_to_ns(value: timespec) -> EResult<usize> {
    if value.tv_sec < 0 || value.tv_nsec < 0 || (value.tv_nsec as usize) >= NS_PER_SEC {
        return Err(Errno::EINVAL);
    }
    let seconds = (value.tv_sec as usize)
        .checked_mul(NS_PER_SEC)
        .ok_or(Errno::EINVAL)?;
    seconds
        .checked_add(value.tv_nsec as usize)
        .ok_or(Errno::EINVAL)
}

const fn ns_to_timespec(value: usize) -> timespec {
    timespec {
        tv_sec: (value / NS_PER_SEC) as _,
        tv_nsec: (value % NS_PER_SEC) as _,
    }
}
