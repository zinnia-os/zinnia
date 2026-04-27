use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    process::{
        Process,
        signal::{Signal, SignalSet},
    },
    sched::Scheduler,
    uapi::signal::{SI_USER, signalfd_siginfo},
    util::mutex::spin::SpinMutex,
    vfs::{
        File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
    },
};
use alloc::sync::Arc;

pub struct SignalfdFile {
    process: Arc<Process>,
    mask: SpinMutex<SignalSet>,
}

impl SignalfdFile {
    pub fn new(process: Arc<Process>, mask: SignalSet) -> Self {
        let mut mask = mask;
        // SIGKILL and SIGSTOP can never be accepted via signalfd.
        mask.sanitize_mask();
        Self {
            process,
            mask: SpinMutex::new(mask),
        }
    }

    /// Dequeue the lowest-numbered pending signal matching the mask from the current task.
    /// Returns the consumed signal, or None if none are pending.
    fn dequeue_one(&self) -> Option<Signal> {
        let mask = *self.mask.lock();
        let task = Scheduler::get_current();
        let mut state = task.signal.lock();
        let pending = state.pending & mask;
        let sig = pending.first_set()?;
        state.pending.set(sig, false);
        Some(sig)
    }

    fn has_pending(&self) -> bool {
        let mask = *self.mask.lock();
        let task = Scheduler::get_current();
        let state = task.signal.lock();
        !(state.pending & mask).is_empty()
    }
}

fn make_siginfo(sig: Signal) -> signalfd_siginfo {
    signalfd_siginfo {
        ssi_signo: sig.as_raw(),
        ssi_code: SI_USER as i32,
        ..Default::default()
    }
}

impl FileOps for SignalfdFile {
    fn read(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        let entry_size = size_of::<signalfd_siginfo>();
        if buf.len() < entry_size {
            return Err(Errno::EINVAL);
        }

        loop {
            // Register as a waiter *before* checking, so a signal delivered between the check and the wait()
            // can't be missed.
            let guard = self.process.signalfd_event.guard();

            if self.has_pending() {
                break;
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            }

            guard.wait();

            // A pending signal that isn't in our mask would normally interrupt a blocking syscall.
            // Return EINTR so userspace can handle it.
            if Scheduler::get_current().has_pending_signals() && !self.has_pending() {
                return Err(Errno::EINTR);
            }
        }

        let max_entries = buf.len() / entry_size;
        let mut written = 0isize;
        for _ in 0..max_entries {
            let Some(sig) = self.dequeue_one() else { break };
            let info = make_siginfo(sig);
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &info as *const signalfd_siginfo as *const u8,
                    entry_size,
                )
            };
            let n = buf.copy_from_slice(bytes)?;
            if (n as usize) < entry_size {
                return Err(Errno::EFAULT);
            }
            written += n;
        }

        Ok(written)
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let mut revents = PollFlags::empty();
        if self.has_pending() {
            revents |= PollFlags::In;
        }
        Ok(revents & mask)
    }

    fn poll_events(&self, _file: &File, _mask: PollFlags) -> PollEventSet<'_> {
        PollEventSet::one(&self.process.signalfd_event)
    }
}
