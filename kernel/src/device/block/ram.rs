//! A block device backed entirely by a region of physical memory.

use super::{BioRequest, BlockDevice, BlockLimits, BlockOp};
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

    fn limits(&self) -> BlockLimits {
        BlockLimits {
            max_lbas: usize::MAX,
            max_segments: usize::MAX,
        }
    }

    fn submit_bio(&self, bio: &Arc<BioRequest>) -> EResult<()> {
        let Some(end_lba) = bio.lba().checked_add(bio.num_lbas() as u64) else {
            bio.complete(Err(Errno::EOVERFLOW));
            return Ok(());
        };
        if end_lba > self.lba_count {
            bio.complete(match bio.op() {
                BlockOp::Read => Ok(0),
                BlockOp::Write => Err(Errno::ENOSPC),
            });
            return Ok(());
        }

        let mut lba = bio.lba() as usize;
        for seg in bio.segments() {
            let store = (self.base + lba * LBA_SIZE).as_hhdm::<u8>();
            let buffer = seg.phys().as_hhdm::<u8>();
            // The backing store and the I/O buffer never overlap.
            unsafe {
                match bio.op() {
                    BlockOp::Read => core::ptr::copy_nonoverlapping(store, buffer, seg.len()),
                    BlockOp::Write => core::ptr::copy_nonoverlapping(buffer, store, seg.len()),
                }
            }
            lba += seg.len() / LBA_SIZE;
        }

        bio.complete(Ok(bio.num_lbas()));
        Ok(())
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
