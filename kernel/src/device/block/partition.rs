use super::{BioRequest, BlockDevice, BlockLimits, BlockOp};
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

    fn limits(&self) -> BlockLimits {
        self.parent.limits()
    }

    fn submit_bio(&self, bio: &Arc<BioRequest>) -> EResult<()> {
        if bio.lba() >= self.lba_count {
            bio.complete(match bio.op() {
                BlockOp::Read => Ok(0),
                BlockOp::Write => Err(Errno::ENOSPC),
            });
            return Ok(());
        }

        let remaining = self.lba_count - bio.lba();
        if bio.op() == BlockOp::Write && bio.num_lbas() as u64 > remaining {
            bio.complete(Err(Errno::ENOSPC));
            return Ok(());
        }

        bio.remap_lba(self.start_lba);
        self.parent.submit_bio(bio)
    }

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.parent.handle_ioctl(file, request, arg)
    }
}

impl Device for PartitionDevice {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(self.clone())
    }

    fn major(&self) -> u32 {
        self.parent.major()
    }

    fn minor(&self) -> u32 {
        self.parent.minor()
    }
}
