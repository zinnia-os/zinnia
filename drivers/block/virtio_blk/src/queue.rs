use crate::{error::BlkError, spec};
use core::{hint::spin_loop, time::Duration};
use virtio::{DescChain, VirtQueue, VirtioDevice};
use zinnia::{
    alloc::{collections::BTreeMap, sync::Arc, vec::Vec},
    arch, clock,
    device::block::BioRequest,
    memory::{AllocFlags, MemoryView, OwnedPhysPages, PhysAddr},
    posix::errno::{EResult, Errno},
    util::{event::Event, mutex::spin::SpinMutex},
};

const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);

struct InFlight {
    slot: u16,
    bio: Arc<BioRequest>,
}

struct Requests {
    pages: OwnedPhysPages,
    count: usize,
    free: Vec<u16>,
    inflight: BTreeMap<DescChain, InFlight>,
}

impl Requests {
    fn new(count: usize) -> Result<Self, BlkError> {
        let page_size = arch::virt::get_page_size();
        let pages = OwnedPhysPages::new(
            (count * spec::req::SIZE).div_ceil(page_size).max(1),
            AllocFlags::empty(),
        )
        .map_err(|_| BlkError::AllocationFailed)?;

        Ok(Self {
            pages,
            count,
            free: (0..count as u16).rev().collect(),
            inflight: BTreeMap::new(),
        })
    }

    fn header_phys(&self, slot: u16) -> PhysAddr {
        self.pages.phys() + slot as usize * spec::req::SIZE
    }

    fn status_phys(&self, slot: u16) -> PhysAddr {
        self.header_phys(slot) + spec::req::STATUS.offset()
    }

    fn slot_bytes(&mut self, slot: u16) -> Option<&mut [u8]> {
        let offset = (slot as usize).checked_mul(spec::req::SIZE)?;
        let arena = unsafe {
            core::slice::from_raw_parts_mut(
                self.pages.as_hhdm::<u8>(),
                self.count * spec::req::SIZE,
            )
        };
        arena.get_mut(offset..offset.checked_add(spec::req::SIZE)?)
    }

    fn write_header(&mut self, slot: u16, kind: u32, sector: u64) -> Option<()> {
        let bytes = self.slot_bytes(slot)?;
        bytes.fill(0);
        bytes.write_reg(spec::req::TYPE, kind)?;
        bytes.write_reg(spec::req::SECTOR, sector)?;
        bytes.write_reg(spec::req::STATUS, spec::status::UNSET)
    }

    fn begin(&mut self, kind: u32, sector: u64) -> Option<u16> {
        let slot = self.free.pop()?;
        match self.write_header(slot, kind, sector) {
            Some(()) => Some(slot),
            None => {
                self.free.push(slot);
                None
            }
        }
    }

    fn status(&mut self, slot: u16) -> u8 {
        self.slot_bytes(slot)
            .and_then(|bytes| bytes.read_reg(spec::req::STATUS))
            .map(|status| status.value())
            .unwrap_or(spec::status::UNSET)
    }

    fn release(&mut self, slot: u16) {
        self.free.push(slot);
    }
}

struct Inner {
    device: VirtioDevice,
    queue: VirtQueue,
    requests: Requests,
}

pub struct RequestQueue {
    inner: SpinMutex<Inner>,
    completions: Event,
    polling: bool,
}

impl RequestQueue {
    pub fn new(
        device: VirtioDevice,
        mut queue: VirtQueue,
        polling: bool,
    ) -> Result<Self, BlkError> {
        queue.set_no_interrupt(polling);
        let requests = Requests::new(queue.queue_size() as usize)?;

        Ok(Self {
            inner: SpinMutex::new(Inner {
                device,
                queue,
                requests,
            }),
            completions: Event::new(),
            polling,
        })
    }

    pub fn submit(&self, bio: &Arc<BioRequest>, kind: u32, sector: u64) -> Result<(), BlkError> {
        let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);

        loop {
            if self.try_submit(bio, kind, sector)? {
                return Ok(());
            }

            if self.drain() > 0 {
                continue;
            }

            if self.inflight() == 0 {
                return Err(BlkError::QueueFull);
            }

            if clock::get_elapsed() >= deadline {
                return Err(BlkError::Timeout);
            }

            if self.polling {
                spin_loop();
            } else if let Some(guard) = self.completions.guard_if(|| self.inflight() > 0) {
                guard.wait();
            }
        }
    }

    pub fn drain(&self) -> usize {
        let mut completed = 0;

        {
            let mut inner = self.inner.lock();

            while let Some(used) = inner.queue.get_used() {
                inner.queue.release_used_chain(used.chain);

                let Some(request) = inner.requests.inflight.remove(&used.chain) else {
                    continue;
                };

                let status = inner.requests.status(request.slot);
                inner.requests.release(request.slot);
                request
                    .bio
                    .complete(outcome(status, request.bio.num_lbas()));
                completed += 1;
            }
        }

        if completed > 0 {
            self.completions.wake_all();
        }

        completed
    }

    pub fn wait_if_polling(&self, bio: &Arc<BioRequest>) {
        if !self.polling {
            return;
        }

        let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);
        while !bio.is_done() {
            self.drain();
            if bio.is_done() {
                break;
            }
            if clock::get_elapsed() >= deadline {
                bio.complete(Err(Errno::EIO));
                break;
            }
            spin_loop();
        }
    }

    fn try_submit(&self, bio: &Arc<BioRequest>, kind: u32, sector: u64) -> Result<bool, BlkError> {
        let writable = kind == spec::req_type::IN;
        let mut buffers: Vec<(PhysAddr, usize, bool)> =
            Vec::with_capacity(bio.segments().len() + 2);

        let mut inner = self.inner.lock();

        let Some(slot) = inner.requests.begin(kind, sector) else {
            return Ok(false);
        };

        buffers.push((
            inner.requests.header_phys(slot),
            spec::req::HEADER_LEN,
            false,
        ));
        for segment in bio.segments() {
            buffers.push((segment.phys(), segment.len(), writable));
        }
        buffers.push((
            inner.requests.status_phys(slot),
            spec::req::STATUS_LEN,
            true,
        ));

        let Ok(chain) = inner.queue.add_buffer(&buffers) else {
            inner.requests.release(slot);
            return Ok(false);
        };

        inner.requests.inflight.insert(
            chain,
            InFlight {
                slot,
                bio: bio.clone(),
            },
        );

        inner
            .device
            .notify_queue(&inner.queue)
            .map_err(|_| BlkError::NotifyFailed)?;

        Ok(true)
    }

    fn inflight(&self) -> usize {
        self.inner.lock().requests.inflight.len()
    }
}

fn outcome(status: u8, lbas: usize) -> EResult<usize> {
    match status {
        spec::status::OK => Ok(lbas),
        spec::status::UNSUPP => Err(Errno::ENOTSUP),
        _ => Err(Errno::EIO),
    }
}
