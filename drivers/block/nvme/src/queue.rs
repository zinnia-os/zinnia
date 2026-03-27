use crate::{
    command::{Command, CreateCQCommand, CreateSQCommand},
    error::NvmeError,
    spec::{self, CompletionEntry, CompletionStatus},
};
use zinnia::{
    alloc::sync::Arc,
    log,
    memory::{
        AllocFlags, KernelAlloc, MmioView, PageAllocator, PhysAddr, Register, UnsafeMemoryView,
    },
};

const DOORBELL_OFFSET: usize = 0x1000;
const TAIL_DOORBELL: Register<u32> = Register::new(0);
const HEAD_DOORBELL: Register<u32> = Register::new(4);

pub struct Queue {
    queue_id: usize,
    /// Amount of queue entries.
    depth: usize,
    doorbells_offset: usize,
    regs: Arc<MmioView>,
    /// Physical buffer for the completion queue.
    cq_addr: PhysAddr,
    cq_view: MmioView,
    /// The index of the current completion queue entry.
    cq_head: usize,
    /// Determines whether a completion queue entry is new.
    cq_phase: u8,
    /// Physical buffer for the submission queue.
    sq_addr: PhysAddr,
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
        let cq_addr = KernelAlloc::alloc_bytes(cq_size as _, AllocFlags::empty())
            .map_err(|_| NvmeError::AllocationFailed)?;
        let cq_view = unsafe { MmioView::new(cq_addr, cq_size as _) };

        // Allocate memory for the submission queue.
        let sq_addr = KernelAlloc::alloc_bytes(sq_size as _, AllocFlags::empty())
            .map_err(|_| NvmeError::AllocationFailed)?;
        let sq_view = unsafe { MmioView::new(sq_addr, sq_size as _) };

        // Calculate the offset of the doorbell registers. The stride is already precomputed here.
        let doorbells_offset = DOORBELL_OFFSET + (queue_id * 2 * doorbell_stride);

        log!("Created queue {queue_id}: sq_size = {sq_size}, cq_size = {cq_size}");

        Ok(Self {
            queue_id,
            depth,
            regs,
            doorbells_offset,
            cq_view,
            cq_addr,
            cq_head: 0,
            cq_phase: 1, // When the controller is enabled, the first phase is 1.
            sq_view,
            sq_addr,
            sq_tail: 0,
        })
    }

    /// Registers a queue as an IO queue.
    pub fn setup_io(&self, admin_queue: &mut Queue) -> Result<(), NvmeError> {
        log!("Setting up queue {} for IO", self.get_id());
        admin_queue.submit_cmd(CreateCQCommand {
            queue: self,
            irqs_enabled: false, // TODO: Enable interrupts.
            irq_vector: 0,       // TODO: Give this a proper IRQ.
        })?;

        let completion = admin_queue.next_completion()?;
        assert!(completion.status.is_success());

        admin_queue.submit_cmd(CreateSQCommand { queue: self })?;

        let completion = admin_queue.next_completion()?;
        assert!(completion.status.is_success());

        Ok(())
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
        loop {
            dw3 = unsafe {
                view.read_reg(spec::cq_entry::DW3)
                    .ok_or(NvmeError::MmioFailed)?
            };

            // The controller will flip the phase bit of the current entry when writing.
            if dw3.read_field(spec::cq_entry::PHASE_TAG).value() == self.cq_phase {
                break;
            }
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
        self.sq_addr
    }

    pub fn get_cq_addr(&self) -> PhysAddr {
        self.cq_addr
    }

    pub fn get_depth(&self) -> usize {
        self.depth
    }

    pub fn get_id(&self) -> usize {
        self.queue_id
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        unsafe {
            KernelAlloc::dealloc_bytes(self.sq_addr, self.sq_view.len());
            KernelAlloc::dealloc_bytes(self.cq_addr, self.cq_view.len());
        }
    }
}
