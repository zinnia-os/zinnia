use crate::{
    arch::virt::get_page_size,
    device::block::{BlockOp, BlockSegment},
    posix::errno::{EResult, Errno},
    util::{event::Event, mutex::spin::SpinMutex},
};
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy)]
pub struct BlockLimits {
    pub max_lbas: usize,
    pub max_segments: usize,
}

pub struct BioRequest {
    op: BlockOp,
    lba: AtomicU64,
    num_lbas: usize,
    lba_size: usize,
    segments: Vec<BlockSegment>,
    done: AtomicBool,
    result: SpinMutex<Option<EResult<usize>>>,
    event: Event,
}

impl BioRequest {
    pub fn new(
        op: BlockOp,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
        segments: Vec<BlockSegment>,
    ) -> EResult<Arc<Self>> {
        if lba_size == 0 || num_lbas == 0 {
            return Err(Errno::EINVAL);
        }

        let page_size = get_page_size();
        if lba_size > page_size {
            return Err(Errno::EINVAL);
        }

        let want = num_lbas.checked_mul(lba_size).ok_or(Errno::EOVERFLOW)?;
        let mut total = 0usize;
        let last = segments.len().wrapping_sub(1);
        for (i, seg) in segments.iter().enumerate() {
            if seg.is_empty() || !seg.len().is_multiple_of(lba_size) {
                return Err(Errno::EINVAL);
            }
            let start_aligned = seg.phys().value().is_multiple_of(page_size);
            let end_aligned = (seg.phys().value() + seg.len()).is_multiple_of(page_size);
            if i != 0 && !start_aligned {
                return Err(Errno::EINVAL);
            }
            if i != last && !end_aligned {
                return Err(Errno::EINVAL);
            }
            total += seg.len();
        }
        if total != want {
            return Err(Errno::EINVAL);
        }

        Ok(Arc::new(Self {
            op,
            lba: AtomicU64::new(lba),
            num_lbas,
            lba_size,
            segments,
            done: AtomicBool::new(false),
            result: SpinMutex::new(None),
            event: Event::new(),
        }))
    }

    pub fn op(&self) -> BlockOp {
        self.op
    }

    pub fn lba(&self) -> u64 {
        self.lba.load(Ordering::Relaxed)
    }

    pub fn num_lbas(&self) -> usize {
        self.num_lbas
    }

    pub fn lba_size(&self) -> usize {
        self.lba_size
    }

    pub fn bytes(&self) -> usize {
        self.num_lbas * self.lba_size
    }

    pub fn segments(&self) -> &[BlockSegment] {
        &self.segments
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Offsets the target LBA, used to translate partition-relative requests.
    pub fn remap_lba(&self, delta: u64) {
        self.lba.fetch_add(delta, Ordering::Relaxed);
    }

    /// Records the LBAs transferred and wakes any waiter.
    pub fn complete(&self, result: EResult<usize>) {
        {
            let mut slot = self.result.lock();
            if slot.is_none() {
                *slot = Some(result);
            }
        }
        self.done.store(true, Ordering::Release);
        self.event.wake_all();
    }

    /// Blocks until the request completes, returning the LBAs transferred.
    pub fn wait(&self) -> EResult<usize> {
        loop {
            match self.event.guard_if(|| !self.done.load(Ordering::Acquire)) {
                Some(guard) => guard.wait(),
                None => break,
            }
        }
        self.result
            .lock()
            .clone()
            .expect("bio completed without a result")
    }
}
