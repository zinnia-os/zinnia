//! A block device backed entirely by a region of physical memory.

use super::{BlockCompletion, BlockDevice, BlockIo, BlockOp};
use crate::{
    device::Device,
    memory::PhysAddr,
    posix::errno::{EResult, Errno},
    vfs::file::{FileOps, OpenFlags},
};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

const RAM_MAJOR: u32 = 1;
const LBA_SIZE: usize = 512;

static NEXT_MINOR: AtomicU32 = AtomicU32::new(0);

pub struct RamDisk {
    base: PhysAddr,
    lba_count: u64,
    minor: u32,
}

impl RamDisk {
    /// Creates a RAM disk over `len` bytes of physical memory starting at `base`.
    /// Any trailing bytes that don't make up a whole sector are ignored.
    pub fn new(base: PhysAddr, len: usize) -> Self {
        Self {
            base,
            lba_count: (len / LBA_SIZE) as u64,
            minor: NEXT_MINOR.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl BlockDevice for RamDisk {
    fn get_lba_size(&self) -> usize {
        LBA_SIZE
    }

    fn lba_count(&self) -> u64 {
        self.lba_count
    }

    fn submit_io(&self, io: &mut BlockIo) -> EResult<BlockCompletion> {
        let Some(end_lba) = io.lba().checked_add(io.num_lbas() as u64) else {
            return Err(Errno::EOVERFLOW);
        };

        if end_lba > self.lba_count {
            return match io.op() {
                BlockOp::Read => Ok(BlockCompletion { lbas: 0 }),
                BlockOp::Write => Err(Errno::ENOSPC),
            };
        }

        let bytes = io.num_lbas() * LBA_SIZE;
        let segment = io.first_segment();

        let store = (self.base + io.lba() as usize * LBA_SIZE).as_hhdm::<u8>();
        let buffer = segment.phys().as_hhdm::<u8>();

        // The backing store and the I/O buffer never overlap.
        unsafe {
            match io.op() {
                BlockOp::Read => core::ptr::copy_nonoverlapping(store, buffer, bytes),
                BlockOp::Write => core::ptr::copy_nonoverlapping(buffer, store, bytes),
            }
        }

        Ok(BlockCompletion {
            lbas: io.num_lbas(),
        })
    }
}

impl Device for RamDisk {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(self.clone())
    }

    fn major(&self) -> u32 {
        RAM_MAJOR
    }

    fn minor(&self) -> u32 {
        self.minor
    }
}
