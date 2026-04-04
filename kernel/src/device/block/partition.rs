use super::BlockDevice;
use crate::{
    memory::{PhysAddr, VirtAddr},
    posix::errno::{EResult, Errno},
    vfs::File,
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

    fn read_lba(&self, buffer: PhysAddr, num_lba: usize, lba: u64) -> EResult<usize> {
        if lba >= self.lba_count {
            return Ok(0);
        }
        let clamped = (num_lba as u64).min(self.lba_count - lba) as usize;
        self.parent.read_lba(buffer, clamped, self.start_lba + lba)
    }

    fn write_lba(&self, buffer: PhysAddr, lba: u64) -> EResult<()> {
        if lba >= self.lba_count {
            return Err(Errno::ENOSPC);
        }
        self.parent.write_lba(buffer, self.start_lba + lba)
    }

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.parent.handle_ioctl(file, request, arg)
    }
}
