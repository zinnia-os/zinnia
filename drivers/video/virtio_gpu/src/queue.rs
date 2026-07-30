use crate::{
    command::{Command, DestroyContext, DetachBacking, UnrefResource},
    dma::DmaRegion,
    error::GpuError,
    spec,
};
use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use virtio::{DescChain, VirtQueue, VirtioDevice};
use zinnia::{
    alloc::{collections::BTreeMap, sync::Arc, vec::Vec},
    clock,
    core::time::Duration,
    memory::{MemoryView, PhysAddr},
    util::{event::Event, mutex::spin::SpinMutex},
};

const SLOT_COUNT: usize = 16;
const SLOT_REQUEST_SIZE: usize = 256;
const SLOT_RESPONSE_SIZE: usize = 4096;
const CURSOR_POOL_SIZE: usize = 8;
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fencing {
    Unfenced,
    Fenced,
}

struct Slot {
    request: DmaRegion,
    response: DmaRegion,
}

impl Slot {
    fn new() -> Result<Self, GpuError> {
        Ok(Self {
            request: DmaRegion::new(SLOT_REQUEST_SIZE)?,
            response: DmaRegion::new(SLOT_RESPONSE_SIZE)?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingRequest {
    ticket: u64,
    fenced: bool,
}

struct SlotTable {
    free: Vec<Slot>,
    pending: BTreeMap<DescChain, PendingRequest>,
    completed: BTreeMap<u64, u32>,
}

pub enum Retired {
    Resource { id: u32, backing: Option<DmaRegion> },
    Context { id: u32 },
}

pub struct ControlQueue {
    virtio: Arc<SpinMutex<VirtioDevice>>,
    queue: SpinMutex<VirtQueue>,
    slots: SpinMutex<SlotTable>,
    graveyard: SpinMutex<Vec<Retired>>,
    slot_available: Event,
    completions: Event,
    polling: bool,
    poisoned: AtomicBool,
    next_ticket: AtomicU64,
    signaled_fence: AtomicU64,
}

impl ControlQueue {
    pub fn new(
        virtio: Arc<SpinMutex<VirtioDevice>>,
        queue: VirtQueue,
        polling: bool,
    ) -> Result<Self, GpuError> {
        let mut free = Vec::with_capacity(SLOT_COUNT);
        for _ in 0..SLOT_COUNT {
            free.push(Slot::new()?);
        }

        Ok(Self {
            virtio,
            queue: SpinMutex::new(queue),
            slots: SpinMutex::new(SlotTable {
                free,
                pending: BTreeMap::new(),
                completed: BTreeMap::new(),
            }),
            graveyard: SpinMutex::new(Vec::new()),
            slot_available: Event::new(),
            completions: Event::new(),
            polling,
            poisoned: AtomicBool::new(false),
            next_ticket: AtomicU64::new(1),
            signaled_fence: AtomicU64::new(0),
        })
    }

    pub fn signaled_fence(&self) -> u64 {
        self.signaled_fence.load(Ordering::Acquire)
    }

    pub fn retire(&self, object: Retired) {
        self.graveyard.lock().push(object);
    }

    fn reclaim(&self) {
        let retired = core::mem::take(&mut *self.graveyard.lock());

        for object in retired {
            match object {
                Retired::Resource { id, backing } => {
                    if backing.is_some() {
                        let _ = self.execute_checked(
                            0,
                            &DetachBacking { resource_id: id },
                            Fencing::Unfenced,
                        );
                    }
                    let _ = self.execute_checked(
                        0,
                        &UnrefResource { resource_id: id },
                        Fencing::Unfenced,
                    );
                    drop(backing);
                }
                Retired::Context { id } => {
                    let _ = self.execute_checked(id, &DestroyContext, Fencing::Unfenced);
                }
            }
        }
    }

    pub fn execute(
        &self,
        ctx_id: u32,
        cmd: &impl Command,
        fencing: Fencing,
        response: &mut [u8],
    ) -> Result<u64, GpuError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(GpuError::Timeout);
        }

        self.reclaim();

        let mut slot = self.acquire_slot()?;
        match self.execute_with_slot(&mut slot, ctx_id, cmd, fencing, response) {
            Err(GpuError::Timeout) => {
                self.poisoned.store(true, Ordering::Release);
                core::mem::forget(slot);
                Err(GpuError::Timeout)
            }
            result => {
                self.release_slot(slot);
                result
            }
        }
    }

    pub fn execute_checked(
        &self,
        ctx_id: u32,
        cmd: &impl Command,
        fencing: Fencing,
    ) -> Result<u64, GpuError> {
        self.execute(ctx_id, cmd, fencing, &mut [])
    }

    fn execute_with_slot(
        &self,
        slot: &mut Slot,
        ctx_id: u32,
        cmd: &impl Command,
        fencing: Fencing,
        response: &mut [u8],
    ) -> Result<u64, GpuError> {
        let body_len = cmd.body_len();
        if body_len > slot.request.len() {
            return Err(GpuError::EncodingFailed);
        }

        let response_len = cmd.response_len().max(response.len());
        let mut spill = if response_len > slot.response.len() {
            Some(DmaRegion::new(response_len)?)
        } else {
            None
        };
        let target = spill.as_mut().unwrap_or(&mut slot.response);

        {
            let bytes = target
                .as_mut_slice()
                .get_mut(..spec::ctrl_hdr::SIZE)
                .ok_or(GpuError::EncodingFailed)?;
            bytes.fill(0);
        }

        let ticket = self.next_ticket.fetch_add(1, Ordering::AcqRel);
        let fenced = fencing == Fencing::Fenced;

        {
            let request = slot
                .request
                .as_mut_slice()
                .get_mut(..body_len)
                .ok_or(GpuError::EncodingFailed)?;
            request.fill(0);
            request
                .write_reg(spec::ctrl_hdr::TYPE, cmd.command_type())
                .ok_or(GpuError::EncodingFailed)?;
            request
                .write_reg(
                    spec::ctrl_hdr::FLAGS,
                    if fenced { spec::FLAG_FENCE } else { 0 },
                )
                .ok_or(GpuError::EncodingFailed)?;
            request
                .write_reg(spec::ctrl_hdr::FENCE_ID, if fenced { ticket } else { 0 })
                .ok_or(GpuError::EncodingFailed)?;
            request
                .write_reg(spec::ctrl_hdr::CTX_ID, ctx_id)
                .ok_or(GpuError::EncodingFailed)?;
            cmd.encode_body(request).ok_or(GpuError::EncodingFailed)?;
        }

        let mut buffers: Vec<(PhysAddr, usize, bool)> = Vec::with_capacity(3);
        buffers.push((slot.request.phys(), body_len, false));
        if let Some(payload) = cmd.payload() {
            buffers.push((payload.phys(), payload.len(), false));
        }
        buffers.push((target.phys(), response_len, true));

        {
            let mut queue = self.queue.lock();
            let chain = queue
                .add_buffer(&buffers)
                .map_err(|_| GpuError::QueueFull)?;
            self.slots
                .lock()
                .pending
                .insert(chain, PendingRequest { ticket, fenced });
            self.virtio
                .lock()
                .notify_queue(&queue)
                .map_err(|_| GpuError::NotifyFailed)?;
        }

        let written = match self.wait_for(ticket) {
            Ok(written) => written as usize,
            Err(error) => {
                if let Some(spill) = spill.take() {
                    core::mem::forget(spill);
                }
                return Err(error);
            }
        };

        let target = spill.as_ref().unwrap_or(&slot.response);

        let header = target
            .as_slice()
            .get(..spec::ctrl_hdr::SIZE)
            .ok_or(GpuError::ShortResponse)?;
        let type_ = header
            .read_reg(spec::ctrl_hdr::TYPE)
            .ok_or(GpuError::ShortResponse)?
            .value();

        if !matches!(
            type_,
            spec::resp::OK_NODATA
                | spec::resp::OK_DISPLAY_INFO
                | spec::resp::OK_CAPSET_INFO
                | spec::resp::OK_CAPSET
        ) {
            return Err(GpuError::from_response(type_));
        }

        let available = written.min(response_len).min(response.len());
        if !response.is_empty() {
            let source = target
                .as_slice()
                .get(..available)
                .ok_or(GpuError::ShortResponse)?;
            response[..available].copy_from_slice(source);
            response[available..].fill(0);
        }

        Ok(if fenced { ticket } else { 0 })
    }

    pub fn wait_fence(&self, fence_id: u64) -> Result<(), GpuError> {
        if fence_id == 0 {
            return Ok(());
        }

        let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);
        loop {
            if self.signaled_fence() >= fence_id {
                return Ok(());
            }

            self.drain();
            if self.signaled_fence() >= fence_id {
                return Ok(());
            }

            if clock::get_elapsed() >= deadline {
                return Err(GpuError::Timeout);
            }

            if self.polling {
                spin_loop();
            } else if let Some(guard) = self
                .completions
                .guard_if(|| self.signaled_fence() < fence_id)
            {
                guard.wait();
            }
        }
    }

    pub fn drain(&self) -> usize {
        let mut woken = 0;

        {
            let mut queue = self.queue.lock();
            let mut slots = self.slots.lock();

            while let Some(used) = queue.get_used() {
                queue.release_used_chain(used.chain);
                let Some(request) = slots.pending.remove(&used.chain) else {
                    continue;
                };
                slots.completed.insert(request.ticket, used.len);
                if request.fenced {
                    self.signaled_fence
                        .fetch_max(request.ticket, Ordering::AcqRel);
                }
                woken += 1;
            }
        }

        if woken > 0 {
            self.completions.wake_all();
        }

        woken
    }

    fn take_completion(&self, ticket: u64) -> Option<u32> {
        self.slots.lock().completed.remove(&ticket)
    }

    fn wait_for(&self, ticket: u64) -> Result<u32, GpuError> {
        let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);

        loop {
            if let Some(len) = self.take_completion(ticket) {
                return Ok(len);
            }

            self.drain();
            if let Some(len) = self.take_completion(ticket) {
                return Ok(len);
            }

            if clock::get_elapsed() >= deadline {
                return Err(GpuError::Timeout);
            }

            if self.polling {
                spin_loop();
            } else if let Some(guard) = self
                .completions
                .guard_if(|| !self.slots.lock().completed.contains_key(&ticket))
            {
                guard.wait();
            }
        }
    }

    fn acquire_slot(&self) -> Result<Slot, GpuError> {
        let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);

        loop {
            if let Some(slot) = self.slots.lock().free.pop() {
                return Ok(slot);
            }

            self.drain();

            if clock::get_elapsed() >= deadline {
                return Err(GpuError::Timeout);
            }

            if self.polling {
                spin_loop();
            } else if let Some(guard) = self
                .slot_available
                .guard_if(|| self.slots.lock().free.is_empty())
            {
                guard.wait();
            }
        }
    }

    fn release_slot(&self, slot: Slot) {
        self.slots.lock().free.push(slot);
        self.slot_available.wake_all();
    }
}

pub struct CursorQueue {
    virtio: Arc<SpinMutex<VirtioDevice>>,
    queue: SpinMutex<VirtQueue>,
    inflight: SpinMutex<BTreeMap<DescChain, DmaRegion>>,
    free: SpinMutex<Vec<DmaRegion>>,
}

impl CursorQueue {
    pub fn new(virtio: Arc<SpinMutex<VirtioDevice>>, queue: VirtQueue) -> Self {
        Self {
            virtio,
            queue: SpinMutex::new(queue),
            inflight: SpinMutex::new(BTreeMap::new()),
            free: SpinMutex::new(Vec::new()),
        }
    }

    pub fn reap(&self) {
        let mut queue = self.queue.lock();

        while let Some(used) = queue.get_used() {
            queue.release_used_chain(used.chain);
            let Some(region) = self.inflight.lock().remove(&used.chain) else {
                continue;
            };
            let mut free = self.free.lock();
            if free.len() < CURSOR_POOL_SIZE {
                free.push(region);
            }
        }
    }

    pub fn submit(&self, cmd: &dyn Command) -> Result<(), GpuError> {
        self.reap();

        let body_len = cmd.body_len();
        let mut region = self.free.lock().pop();
        if region.as_ref().is_none_or(|r| r.len() < body_len) {
            region = Some(DmaRegion::new(SLOT_REQUEST_SIZE.max(body_len))?);
        }
        let mut region = region.ok_or(GpuError::AllocationFailed)?;

        {
            let bytes = region
                .as_mut_slice()
                .get_mut(..body_len)
                .ok_or(GpuError::EncodingFailed)?;
            bytes.fill(0);
            bytes
                .write_reg(spec::ctrl_hdr::TYPE, cmd.command_type())
                .ok_or(GpuError::EncodingFailed)?;
            cmd.encode_body(bytes).ok_or(GpuError::EncodingFailed)?;
        }

        let mut queue = self.queue.lock();
        let chain = queue
            .add_buffer(&[(region.phys(), body_len, false)])
            .map_err(|_| GpuError::QueueFull)?;
        self.inflight.lock().insert(chain, region);
        self.virtio
            .lock()
            .notify_queue(&queue)
            .map_err(|_| GpuError::NotifyFailed)
    }
}
