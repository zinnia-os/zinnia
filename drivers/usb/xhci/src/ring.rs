use crate::spec::{self, TrbType, trb};
use zinnia::{
    alloc::{sync::Arc, vec::Vec},
    arch,
    memory::{AllocFlags, BitValue, MemoryView, OwnedPhysPages, PhysAddr},
    posix::errno::EResult,
    util::{event::Event, mutex::spin::SpinMutex},
};

use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy)]
pub struct Completion {
    /// Raw xHCI completion code [`spec::CompletionCode`].
    pub code: u8,
    /// Bytes transferred or the assigned slot id.
    pub value: u32,
}

/// A one-shot completion shared between the submitter and the event-ring IRQ handler.
pub struct CompletionCell {
    event: Event,
    done: AtomicBool,
    result: SpinMutex<Option<Completion>>,
}

impl CompletionCell {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            event: Event::new(),
            done: AtomicBool::new(false),
            result: SpinMutex::new(None),
        })
    }

    pub fn complete(&self, code: u8, value: u32) {
        {
            let mut result = self.result.lock();
            if result.is_none() {
                *result = Some(Completion { code, value });
            }
        }
        self.done.store(true, Ordering::Release);
        self.event.wake_all();
    }

    pub fn wait(&self) -> Completion {
        loop {
            // Register a waiter only while still pending (register-before-check).
            match self.event.guard_if(|| !self.done.load(Ordering::Acquire)) {
                Some(guard) => guard.wait(),
                None => break,
            }
        }
        self.result.lock().expect("completion done without result")
    }
}

pub struct Ring {
    pages: OwnedPhysPages,
    /// Per slot completions, indexed by TRB slot. [`None`] for the event ring.
    pending: Vec<Option<Arc<CompletionCell>>>,
    /// Ring size in TRBs.
    pub size: usize,
    /// Index of the next TRB to produce/consume.
    pub index: usize,
    /// Producer/consumer cycle state.
    pub cycle: bool,
}

impl Ring {
    pub fn new() -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        let pages = OwnedPhysPages::new(1, AllocFlags::empty())?;

        let size = page_size / spec::TRB_SIZE;
        let mut pending = Vec::new();
        pending.resize_with(size, || None);

        Ok(Self {
            size,
            pages,
            pending,
            index: 0,
            cycle: true,
        })
    }

    pub fn phys(&self) -> PhysAddr {
        self.pages.phys()
    }

    /// Physical address of the TRB at `index`.
    pub fn trb_phys(&self, index: usize) -> PhysAddr {
        PhysAddr::new(self.pages.phys().value() + index * spec::TRB_SIZE)
    }

    fn write_trb(&mut self, index: usize, parameter: u64, status: u32, control: u32) {
        let slice = unsafe {
            core::slice::from_raw_parts_mut(
                self.pages.as_hhdm::<u8>().add(index * spec::TRB_SIZE),
                spec::TRB_SIZE,
            )
        };
        // Parameter and status are written before control so that the cycle bit
        // (in control, the last word) publishes a fully-written TRB.
        slice.write_reg(trb::PARAMETER, parameter);
        slice.write_reg(trb::STATUS, status);
        slice.write_reg(trb::CONTROL, control);
    }

    pub fn enqueue(&mut self, parameter: u64, status: u32, control: u32) -> usize {
        // The last slot is reserved for the Link TRB.
        if self.index == self.size - 1 {
            let link = BitValue::new(0u32)
                .write_field(trb::control::TRB_TYPE, TrbType::Link as u8)
                .write_field(trb::control::TC, 1)
                .write_field(trb::control::C, self.cycle as u8)
                .value();
            let base = self.phys().value() as u64;
            self.write_trb(self.index, base, 0, link);
            self.index = 0;
            self.cycle = !self.cycle;
        }

        let index = self.index;
        let control = control
            | BitValue::new(0u32)
                .write_field(trb::control::C, self.cycle as u8)
                .value();
        self.write_trb(index, parameter, status, control);
        self.index += 1;
        index
    }

    pub fn dequeue(&mut self) -> Option<(u64, u32, u32)> {
        let slice = unsafe {
            core::slice::from_raw_parts(
                self.pages.as_hhdm::<u8>().add(self.index * spec::TRB_SIZE),
                spec::TRB_SIZE,
            )
        };
        let control = slice.read_reg(trb::CONTROL).unwrap().value();
        if ((control & 1) != 0) != self.cycle {
            return None;
        }
        let parameter = slice.read_reg(trb::PARAMETER).unwrap().value();
        let status = slice.read_reg(trb::STATUS).unwrap().value();

        self.index += 1;
        if self.index == self.size {
            self.index = 0;
            self.cycle = !self.cycle;
        }

        Some((parameter, status, control))
    }

    pub fn set_pending(&mut self, index: usize, cell: Arc<CompletionCell>) {
        self.pending[index] = Some(cell);
    }

    pub fn take_pending(&mut self, index: usize) -> Option<Arc<CompletionCell>> {
        self.pending[index].take()
    }

    /// Maps a TRB physical address back to its ring slot index, if it lies in this ring.
    pub fn index_of_phys(&self, phys: u64) -> Option<usize> {
        let base = self.pages.phys().value() as u64;
        let end = base + (self.size * spec::TRB_SIZE) as u64;
        if phys < base || phys >= end {
            return None;
        }
        Some(((phys - base) / spec::TRB_SIZE as u64) as usize)
    }
}
