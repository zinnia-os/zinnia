use crate::{
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    uapi,
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{
        File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
    },
};
use core::hint::unlikely;

#[derive(Debug)]
pub struct PipeBuffer {
    // Using a spin mutex here is fine because the tasks are preempted by the events.
    inner: SpinMutex<PipeInner>,
    rd_queue: Event,
    wr_queue: Event,
}

#[derive(Debug)]
struct PipeInner {
    buffer: RingBuffer,
    readers: usize,
    writers: usize,
}

impl PipeBuffer {
    pub fn new() -> Self {
        Self {
            inner: SpinMutex::new(PipeInner {
                buffer: RingBuffer::new(0x1000),
                readers: 0,
                writers: 0,
            }),
            rd_queue: Event::new(),
            wr_queue: Event::new(),
        }
    }

    /// Returns the capacity of the pipe in bytes.
    pub fn capacity(&self) -> usize {
        self.inner.lock().buffer.capacity()
    }
}

impl FileOps for PipeBuffer {
    fn acquire(&self, _file: &File, flags: OpenFlags) -> EResult<()> {
        let mut inner = self.inner.lock();

        if flags.contains(OpenFlags::Read) {
            inner.readers += 1;
        }
        if flags.contains(OpenFlags::Write) {
            inner.writers += 1;
        }

        Ok(())
    }

    fn release(&self, file: &File) -> EResult<()> {
        {
            let mut inner = self.inner.lock();
            let flags = *file.flags.lock();

            if flags.contains(OpenFlags::Read) {
                inner.readers -= 1;
            }
            if flags.contains(OpenFlags::Write) {
                inner.writers -= 1;
            }
        }

        // Wake blocked readers/writers so they can observe the closed state.
        // A reader blocked on an empty pipe needs to see writers == 0 (EOF).
        // A writer blocked on a full pipe needs to see readers == 0 (EPIPE).
        self.rd_queue.wake_all();
        self.wr_queue.wake_all();

        Ok(())
    }

    fn read(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        if unlikely(buf.is_empty()) {
            return Ok(0);
        }

        let read = self.rd_queue.guard();
        loop {
            let mut inner = self.inner.lock();
            let mut v = vec![0u8; buf.len()];
            let len = inner.buffer.read(&mut v);
            buf.copy_from_slice(&v[..len])?;

            // If there was at least one byte written to the pipe
            if len > 0 {
                self.wr_queue.wake_one();
                return Ok(len as _);
            }

            if inner.writers == 0 {
                return Ok(0);
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            } else {
                drop(inner);
                read.wait();
                if crate::sched::Scheduler::get_current().has_pending_signals() {
                    return Err(Errno::EINTR);
                }
            }
        }
    }

    fn write(&self, file: &File, buf: &mut IovecIter, _off: u64) -> EResult<isize> {
        if unlikely(buf.is_empty()) {
            return Ok(0);
        }

        let write = self.wr_queue.guard();
        loop {
            let len = {
                let mut inner = self.inner.lock();

                if inner.readers == 0 {
                    // TODO: Kill
                    return Err(Errno::EPIPE);
                }

                let to_write = buf.len().min(inner.buffer.get_available_len());
                let mut v = vec![0u8; to_write];
                buf.copy_to_slice(&mut v)?;
                inner.buffer.write(&v)
            };
            if len > 0 {
                self.rd_queue.wake_one();
                return Ok(len as _);
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            } else {
                write.wait();
                if crate::sched::Scheduler::get_current().has_pending_signals() {
                    return Err(Errno::EINTR);
                }
            }
        }
    }

    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let inner = self.inner.lock();
        let flags = *file.flags.lock();
        let mut revents = PollFlags::empty();

        if flags.contains(OpenFlags::Read) {
            // Readable if there is data in the buffer.
            if inner.buffer.get_data_len() > 0 {
                revents |= PollFlags::In;
            }
            // If no writers remain, signal hangup (EOF).
            if inner.writers == 0 {
                revents |= PollFlags::Hup;
            }
        }

        if flags.contains(OpenFlags::Write) {
            // Writable if there is space in the buffer.
            if inner.buffer.get_available_len() > 0 {
                revents |= PollFlags::Out;
            }
            // If no readers remain, signal error (broken pipe).
            if inner.readers == 0 {
                revents |= PollFlags::Err;
            }
        }

        Ok(revents & (mask | PollFlags::Err | PollFlags::Hup))
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        let wants_read = mask.intersects(PollFlags::Read);
        let wants_write = mask.intersects(PollFlags::Write);

        let mut events = PollEventSet::new();
        if wants_read || !wants_write {
            events = events.add(&self.rd_queue);
        }
        if wants_write || !wants_read {
            events = events.add(&self.wr_queue);
        }
        events
    }

    fn ioctl(&self, _file: &File, request: usize, argp: VirtAddr) -> EResult<usize> {
        match request as _ {
            uapi::ioctls::FIONREAD => {
                let len = self.inner.lock().buffer.get_data_len() as i32;
                let mut count_ptr = UserPtr::new(argp);
                count_ptr.write(len).ok_or(Errno::EFAULT)?;
            }
            _ => return Err(Errno::ENOTTY),
        }
        Ok(0)
    }
}
