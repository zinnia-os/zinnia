use super::{BlockCompletion, BlockDevice, BlockIo, BlockOp};
use crate::{
    device::Device,
    memory::VirtAddr,
    posix::errno::{EResult, Errno},
    vfs::{
        File,
        file::{FileOps, OpenFlags},
    },
};
use alloc::sync::Arc;

/// A block device that represents a partition on a parent device.
/// Offsets all LBA addresses by `start_lba` and bounds-checks against `lba_count`.
pub struct PartitionDevice {
    parent: Arc<dyn BlockDevice>,
    start_lba: u64,
    lba_count: u64,
}

impl PartitionDevice {
    pub fn new(parent: Arc<dyn BlockDevice>, start_lba: u64, lba_count: u64) -> Self {
        Self {
            parent,
            start_lba,
            lba_count,
        }
    }
}

impl BlockDevice for PartitionDevice {
    fn get_lba_size(&self) -> usize {
        self.parent.get_lba_size()
    }

    fn lba_count(&self) -> u64 {
        self.lba_count
    }

    fn submit_io(&self, io: &mut BlockIo) -> EResult<BlockCompletion> {
        if io.lba() >= self.lba_count {
            return match io.op() {
                BlockOp::Read => Ok(BlockCompletion { lbas: 0 }),
                BlockOp::Write => Err(Errno::ENOSPC),
            };
        }

        let remaining = self.lba_count - io.lba();
        let num_lbas = io.num_lbas() as u64;
        let forwarded_lba = self
            .start_lba
            .checked_add(io.lba())
            .ok_or(Errno::EOVERFLOW)?;
        let forwarded_lbas = match io.op() {
            BlockOp::Read => num_lbas.min(remaining) as usize,
            BlockOp::Write if num_lbas > remaining => return Err(Errno::ENOSPC),
            BlockOp::Write => io.num_lbas(),
        };

        let segment = io.first_segment();
        let lba_size = self.get_lba_size();
        let mut forwarded = match io.op() {
            BlockOp::Read => BlockIo::read_phys(
                segment.phys(),
                segment.len(),
                forwarded_lba,
                forwarded_lbas,
                lba_size,
            )?,
            BlockOp::Write => BlockIo::write_phys(
                segment.phys(),
                segment.len(),
                forwarded_lba,
                forwarded_lbas,
                lba_size,
            )?,
        };

        self.parent.submit_io(&mut forwarded)
    }

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.parent.handle_ioctl(file, request, arg)
    }
}

impl Device for PartitionDevice {
    fn open(self: Arc<Self>, flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        self.parent.clone().open(flags)
    }

    fn major(&self) -> u32 {
        self.parent.major()
    }

    fn minor(&self) -> u32 {
        self.parent.minor()
    }
}
