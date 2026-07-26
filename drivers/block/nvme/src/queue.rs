use crate::{
    command::{Command, ReadWriteCommand},
    error::NvmeError,
    prp::PrpList,
    spec::{self, CompletionEntry, CompletionStatus},
};
use core::hint::spin_loop;
use zinnia::{
    alloc::{sync::Arc, vec::Vec},
    clock,
    core::time::Duration,
    device::block::BioRequest,
    irq::lock::IrqLock,
    log,
    memory::{
        AllocFlags, MmioView, OwnedPhysPages, PhysAddr, Register, UnsafeMemoryView, VmCacheType,
    },
    posix::errno::Errno,
    util::{event::Event, mutex::spin::SpinMutex},
};

const DOORBELL_OFFSET: usize = 0x1000;
const TAIL_DOORBELL: Register<u32> = Register::new(0);
const HEAD_DOORBELL: Register<u32> = Register::new(4);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);

/// Allocates enough whole pages to back a queue of `bytes` bytes.
fn alloc_queue(bytes: usize) -> Result<OwnedPhysPages, NvmeError> {
    let pages = bytes.div_ceil(zinnia::arch::virt::get_page_size());
    OwnedPhysPages::new(pages, AllocFlags::empty()).map_err(|_| NvmeError::AllocationFailed)
}

pub struct Queue {
    /// Amount of queue entries.
    depth: usize,
    doorbells_offset: usize,
    regs: Arc<MmioView>,
    /// Physical buffer for the completion queue.
    cq_pages: OwnedPhysPages,
    cq_view: MmioView,
    /// The index of the current completion queue entry.
    cq_head: usize,
    /// Determines whether a completion queue entry is new.
    cq_phase: u8,
    /// Physical buffer for the submission queue.
    sq_pages: OwnedPhysPages,
    sq_view: MmioView,
    /// The index of the current submission queue entry.
    sq_tail: usize,
}

impl Queue {
    /// Creates a new submission and completion queue pair.
    pub fn new(
        regs: Arc<MmioView>,
        doorbell_stride: usize,
        queue_id: usize,
        depth: usize,
    ) -> Result<Self, NvmeError> {
        let align = 0x1000;
        let sq_size = ((depth << 6) + align - 1) & !(align - 1);
        let cq_size = ((depth * (size_of::<CompletionEntry>())) + align - 1) & !(align - 1);
        // Allocate memory the completion queue.
        let cq_pages = alloc_queue(cq_size)?;
        let cq_view = unsafe { MmioView::new(cq_pages.phys(), cq_size, VmCacheType::Uncacheable) };

        // Allocate memory for the submission queue.
        let sq_pages = alloc_queue(sq_size)?;
        let sq_view = unsafe { MmioView::new(sq_pages.phys(), sq_size, VmCacheType::Uncacheable) };

        // Calculate the offset of the doorbell registers. The stride is already precomputed here.
        let doorbells_offset = DOORBELL_OFFSET + (queue_id * 2 * doorbell_stride);

        log!("Created queue {queue_id}: sq_size = {sq_size}, cq_size = {cq_size}");

        Ok(Self {
            depth,
            regs,
            doorbells_offset,
            cq_view,
            cq_pages,
            cq_head: 0,
            cq_phase: 1, // When the controller is enabled, the first phase is 1.
            sq_view,
            sq_pages,
            sq_tail: 0,
        })
    }

    /// Submits a command to this queue.
    pub fn submit_cmd(&mut self, command: impl Command) -> Result<(), NvmeError> {
        // Create a subview into the submission queue at the current tail.
        let view = self
            .sq_view
            .sub_view(self.sq_tail * spec::sq_entry::SIZE)
            .ok_or(NvmeError::MmioFailed)?;

        let doorbells = self
            .regs
            .sub_view(self.doorbells_offset)
            .ok_or(NvmeError::MmioFailed)?;

        unsafe {
            (view.base() as *mut u8).write_bytes(0, spec::sq_entry::SIZE);
            command.write_command(&view)?;
        }

        self.sq_tail += 1;
        if self.sq_tail == self.depth {
            self.sq_tail = 0;
        }

        // Notify the controller of the new tail index.
        unsafe { doorbells.write_reg(TAIL_DOORBELL, self.sq_tail as u32) };

        Ok(())
    }

    /// Reads the next completion entry from the queue.
    pub fn next_completion(&mut self) -> Result<spec::CompletionEntry, NvmeError> {
        // Create a subview into the completion queue at the current head.
        let view = self
            .cq_view
            .sub_view(self.cq_head * spec::cq_entry::SIZE)
            .ok_or(NvmeError::MmioFailed)?;

        let doorbells = self
            .regs
            .sub_view(self.doorbells_offset)
            .ok_or(NvmeError::MmioFailed)?;

        // Wait until the phase for this entry has changed.
        let mut dw3;
        let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);
        loop {
            dw3 = unsafe {
                view.read_reg(spec::cq_entry::DW3)
                    .ok_or(NvmeError::MmioFailed)?
            };

            // The controller will flip the phase bit of the current entry when writing.
            if dw3.read_field(spec::cq_entry::PHASE_TAG).value() == self.cq_phase {
                break;
            }

            if clock::get_elapsed() >= deadline {
                return Err(NvmeError::Timeout);
            }

            spin_loop();
        }

        // Then, read the rest of the completion queue entry.
        let dw0 = unsafe {
            view.read_reg(spec::cq_entry::DW0)
                .ok_or(NvmeError::MmioFailed)?
        };
        let dw2 = unsafe {
            view.read_reg(spec::cq_entry::DW2)
                .ok_or(NvmeError::MmioFailed)?
        };

        let entry = CompletionEntry {
            result: dw0.value(),
            sq_head: dw2.read_field(spec::cq_entry::SQ_HEAD).value(),
            sq_id: dw2.read_field(spec::cq_entry::SQ_IDENT).value(),
            cmd_id: dw3.read_field(spec::cq_entry::CID).value(),
            status: CompletionStatus(dw3.read_field(spec::cq_entry::STATUS).value()),
            phase_tag: dw3.read_field(spec::cq_entry::PHASE_TAG).value() != 0,
        };

        self.cq_head += 1;
        if self.cq_head == self.depth {
            self.cq_head = 0;
            self.cq_phase ^= 1; // Flip the phase bit.
        }

        // Notify the controller of the new head index.
        unsafe { doorbells.write_reg(HEAD_DOORBELL, self.cq_head as u32) };

        Ok(entry)
    }

    pub fn get_sq_addr(&self) -> PhysAddr {
        self.sq_pages.phys()
    }

    pub fn get_cq_addr(&self) -> PhysAddr {
        self.cq_pages.phys()
    }

    pub fn get_depth(&self) -> usize {
        self.depth
    }
}

struct InFlight {
    bio: Arc<BioRequest>,
    lbas: usize,
    _prps: PrpList,
}

struct SqState {
    view: MmioView,
    tail: usize,
}

struct CqState {
    view: MmioView,
    head: usize,
    phase: u8,
}

struct CmdTable {
    slots: Vec<Option<InFlight>>,
    free: Vec<u16>,
}

pub struct IoQueue {
    queue_id: usize,
    depth: usize,
    doorbells_offset: usize,
    regs: Arc<MmioView>,
    poll: bool,
    cq_pages: OwnedPhysPages,
    sq_pages: OwnedPhysPages,
    sq: SpinMutex<SqState>,
    cq: SpinMutex<CqState>,
    cmds: SpinMutex<CmdTable>,
    slots_free: Event,
}

impl IoQueue {
    pub fn new(
        regs: Arc<MmioView>,
        doorbell_stride: usize,
        queue_id: usize,
        depth: usize,
        poll: bool,
    ) -> Result<Self, NvmeError> {
        let align = 0x1000;
        let sq_size = ((depth << 6) + align - 1) & !(align - 1);
        let cq_size = ((depth * size_of::<CompletionEntry>()) + align - 1) & !(align - 1);

        let cq_pages = alloc_queue(cq_size)?;
        let cq_view = unsafe { MmioView::new(cq_pages.phys(), cq_size, VmCacheType::Normal) };

        let sq_pages = alloc_queue(sq_size)?;
        let sq_view = unsafe { MmioView::new(sq_pages.phys(), sq_size, VmCacheType::Normal) };

        let doorbells_offset = DOORBELL_OFFSET + (queue_id * 2 * doorbell_stride);

        let slots = depth - 1;
        let mut free = Vec::with_capacity(slots);
        for i in (0..slots).rev() {
            free.push(i as u16);
        }
        let mut cmd_slots = Vec::with_capacity(slots);
        cmd_slots.resize_with(slots, || None);

        log!("Created I/O queue {queue_id}: depth {depth}, poll {poll}");

        Ok(Self {
            queue_id,
            depth,
            doorbells_offset,
            regs,
            poll,
            cq_pages,
            sq_pages,
            sq: SpinMutex::new(SqState {
                view: sq_view,
                tail: 0,
            }),
            cq: SpinMutex::new(CqState {
                view: cq_view,
                head: 0,
                phase: 1,
            }),
            cmds: SpinMutex::new(CmdTable {
                slots: cmd_slots,
                free,
            }),
            slots_free: Event::new(),
        })
    }

    pub fn get_cq_addr(&self) -> PhysAddr {
        self.cq_pages.phys()
    }

    pub fn get_sq_addr(&self) -> PhysAddr {
        self.sq_pages.phys()
    }

    pub fn get_depth(&self) -> usize {
        self.depth
    }

    pub fn get_id(&self) -> usize {
        self.queue_id
    }

    pub fn submit(&self, bio: &Arc<BioRequest>, prps: PrpList, mut cmd: ReadWriteCommand) {
        let cid = self.acquire_slot();

        {
            let _irq = IrqLock::lock();
            self.cmds.lock().slots[cid as usize] = Some(InFlight {
                bio: bio.clone(),
                lbas: bio.num_lbas(),
                _prps: prps,
            });
        }

        cmd.cid = cid;

        let sent = {
            let _irq = IrqLock::lock();
            let mut sq = self.sq.lock();
            self.write_sqe(&mut sq, &cmd).is_some()
        };

        if !sent {
            let inflight = {
                let _irq = IrqLock::lock();
                let mut cmds = self.cmds.lock();
                let taken = cmds.slots[cid as usize].take();
                cmds.free.push(cid);
                taken
            };
            if let Some(inflight) = inflight {
                inflight.bio.complete(Err(Errno::EIO));
            }
        }
    }

    fn write_sqe(&self, sq: &mut SqState, cmd: &ReadWriteCommand) -> Option<()> {
        let view = sq.view.sub_view(sq.tail * spec::sq_entry::SIZE)?;
        let doorbells = self.regs.sub_view(self.doorbells_offset)?;
        unsafe {
            (view.base() as *mut u8).write_bytes(0, spec::sq_entry::SIZE);
            cmd.write_command(&view).ok()?;
        }
        sq.tail += 1;
        if sq.tail == self.depth {
            sq.tail = 0;
        }
        unsafe { doorbells.write_reg(TAIL_DOORBELL, sq.tail as u32) };
        Some(())
    }

    fn acquire_slot(&self) -> u16 {
        loop {
            {
                let _irq = IrqLock::lock();
                if let Some(cid) = self.cmds.lock().free.pop() {
                    return cid;
                }
            }

            if self.poll {
                self.drain();
                spin_loop();
                continue;
            }

            self.drain();
            if let Some(guard) = self.slots_free.guard_if(|| {
                let _irq = IrqLock::lock();
                self.cmds.lock().free.is_empty()
            }) {
                guard.wait();
            }
        }
    }

    pub fn drain(&self) -> usize {
        let _irq = IrqLock::lock();
        let mut cq = self.cq.lock();
        let mut completed = 0;

        loop {
            let Some(view) = cq.view.sub_view(cq.head * spec::cq_entry::SIZE) else {
                break;
            };
            let Some(dw3) = (unsafe { view.read_reg(spec::cq_entry::DW3) }) else {
                break;
            };
            if dw3.read_field(spec::cq_entry::PHASE_TAG).value() != cq.phase {
                break;
            }

            let cid = dw3.read_field(spec::cq_entry::CID).value();
            let status = CompletionStatus(dw3.read_field(spec::cq_entry::STATUS).value());

            cq.head += 1;
            if cq.head == self.depth {
                cq.head = 0;
                cq.phase ^= 1;
            }

            let inflight = {
                let mut cmds = self.cmds.lock();
                let taken = cmds
                    .slots
                    .get_mut(cid as usize)
                    .and_then(|slot| slot.take());
                if taken.is_some() {
                    cmds.free.push(cid);
                }
                taken
            };

            if let Some(inflight) = inflight {
                let result = if status.is_success() {
                    Ok(inflight.lbas)
                } else {
                    Err(Errno::EIO)
                };
                inflight.bio.complete(result);
            }

            completed += 1;
        }

        if completed > 0
            && let Some(doorbells) = self.regs.sub_view(self.doorbells_offset)
        {
            unsafe { doorbells.write_reg(HEAD_DOORBELL, cq.head as u32) };
        }

        drop(cq);

        if completed > 0 {
            self.slots_free.wake_all();
        }

        completed
    }

    pub fn is_polling(&self) -> bool {
        self.poll
    }
}
