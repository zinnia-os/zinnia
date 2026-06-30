use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    process::{
        Process,
        signal::{PendingQueue, SigInfoData, Signal, SignalSet},
    },
    sched::Scheduler,
    uapi::signal::signalfd_siginfo,
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

    fn dequeue_one(&self) -> Option<(Signal, SigInfoData, PendingQueue)> {
        let mask = *self.mask.lock();
        let task = Scheduler::get_current();
        if Arc::ptr_eq(&task.get_process(), &self.process) {
            if let Some(dequeued) = task.signal.lock().queue.dequeue(mask) {
                return Some((dequeued.0, dequeued.1, PendingQueue::Thread));
            }
        }
        self.process
            .shared_pending
            .lock()
            .dequeue(mask)
            .map(|(sig, info)| (sig, info, PendingQueue::Process))
    }

    fn requeue_one(&self, sig: Signal, info: SigInfoData, queue: PendingQueue) {
        let task = Scheduler::get_current();
        match queue {
            PendingQueue::Thread if Arc::ptr_eq(&task.get_process(), &self.process) => {
                task.signal.lock().queue.queue(sig, info);
            }
            PendingQueue::Thread | PendingQueue::Process => {
                self.process.shared_pending.lock().queue(sig, info);
            }
        }
        self.process.signal_event.wake_all();
    }

    fn has_pending(&self) -> bool {
        let mask = *self.mask.lock();
        if self.process.shared_pending.lock().deliverable(mask) {
            return true;
        }
        // Signals are only visible when the caller belongs to the signalfd's process.
        let task = Scheduler::get_current();
        Arc::ptr_eq(&task.get_process(), &self.process)
            && task.signal.lock().queue.deliverable(mask)
    }
}

fn make_siginfo(sig: Signal, info: SigInfoData) -> signalfd_siginfo {
    signalfd_siginfo {
        ssi_signo: sig as u32,
        ssi_errno: info.errno,
        ssi_code: info.code,
        ssi_pid: info.pid as u32,
        ssi_uid: info.uid,
        ssi_status: info.status,
        ssi_addr: info.addr as u64,
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
            let guard = self.process.signal_event.guard();

            if self.has_pending() {
                break;
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            }

            guard.wait();

            if Scheduler::get_current().has_pending_signals() && !self.has_pending() {
                return Err(Errno::EINTR);
            }
        }

        let max_entries = buf.len() / entry_size;
        let mut written = 0isize;
        for _ in 0..max_entries {
            let Some((sig, queued_info, queue)) = self.dequeue_one() else {
                break;
            };
            let user_info = make_siginfo(sig, queued_info);
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &user_info as *const signalfd_siginfo as *const u8,
                    entry_size,
                )
            };
            let n = match buf.copy_from_slice(bytes) {
                Ok(n) => n,
                Err(err) => {
                    self.requeue_one(sig, queued_info, queue);
                    if written > 0 {
                        return Ok(written);
                    }
                    return Err(err);
                }
            };
            if (n as usize) < entry_size {
                self.requeue_one(sig, queued_info, queue);
                if written > 0 {
                    return Ok(written);
                }
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
        PollEventSet::one(&self.process.signal_event)
    }
}
