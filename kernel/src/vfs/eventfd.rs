use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::{
        File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
    },
};

const EFD_SEMAPHORE: u32 = 1;
const EVENTFD_MAX: u64 = u64::MAX - 1;

pub struct EventfdFile {
    counter: SpinMutex<u64>,
    flags: u32,
    event: Event,
}

impl EventfdFile {
    pub fn new(initval: u32, flags: u32) -> Self {
        Self {
            counter: SpinMutex::new(initval as u64),
            flags,
            event: Event::new(),
        }
    }
}

impl FileOps for EventfdFile {
    fn read(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        if buf.len() < size_of::<u64>() {
            return Err(Errno::EINVAL);
        }

        loop {
            let guard = self.event.guard();
            let value = {
                let mut counter = self.counter.lock();
                if *counter != 0 {
                    if self.flags & EFD_SEMAPHORE != 0 {
                        *counter -= 1;
                        1
                    } else {
                        let value = *counter;
                        *counter = 0;
                        value
                    }
                } else {
                    0
                }
            };

            if value != 0 {
                self.event.wake_all();
                let bytes = value.to_ne_bytes();
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

    fn write(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        if buf.len() < size_of::<u64>() {
            return Err(Errno::EINVAL);
        }

        let mut bytes = [0u8; size_of::<u64>()];
        let n = buf.copy_to_slice(&mut bytes)?;
        if (n as usize) < bytes.len() {
            return Err(Errno::EFAULT);
        }
        let value = u64::from_ne_bytes(bytes);
        if value == u64::MAX {
            return Err(Errno::EINVAL);
        }

        loop {
            let guard = self.event.guard();
            {
                let mut counter = self.counter.lock();
                if *counter <= EVENTFD_MAX - value {
                    *counter += value;
                    self.event.wake_all();
                    return Ok(size_of::<u64>() as isize);
                }
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

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let counter = *self.counter.lock();
        let mut revents = PollFlags::empty();
        if counter != 0 {
            revents |= PollFlags::In;
        }
        if counter < EVENTFD_MAX {
            revents |= PollFlags::Out;
        }
        Ok(revents & mask)
    }

    fn poll_events(&self, _file: &File, _mask: PollFlags) -> PollEventSet<'_> {
        PollEventSet::one(&self.event)
    }
}
