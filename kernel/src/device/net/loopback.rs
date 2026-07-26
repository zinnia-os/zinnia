use crate::{
    device::net::{l2::mac::MacAddr, nic::NicDevice},
    posix::errno::EResult,
    util::{event::Event, mutex::spin::SpinMutex},
};
use alloc::{collections::vec_deque::VecDeque, vec::Vec};

#[derive(Debug)]
pub struct LoopbackNic {
    queue: SpinMutex<VecDeque<Vec<u8>>>,
    ready: Event,
}

impl LoopbackNic {
    pub fn new() -> Self {
        Self {
            queue: SpinMutex::new(VecDeque::new()),
            ready: Event::new(),
        }
    }
}

impl NicDevice for LoopbackNic {
    fn mac(&self) -> MacAddr {
        // Loopback has no hardware address.
        MacAddr::ZERO
    }

    fn send(&self, frame: &[u8]) -> EResult<()> {
        self.queue.lock().push_back(frame.to_vec());
        self.ready.wake_all();
        Ok(())
    }

    fn recv(&self, frame: &mut [u8]) -> EResult<usize> {
        loop {
            // Register before checking, so a frame queued between the check and the wait is not missed.
            let guard = self.ready.guard();

            if let Some(queued) = self.queue.lock().pop_front() {
                let len = queued.len().min(frame.len());
                frame[..len].copy_from_slice(&queued[..len]);
                return Ok(len);
            }

            guard.wait();
        }
    }
}
