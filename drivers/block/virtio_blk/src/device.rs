use crate::{error::BlkError, queue::RequestQueue, spec};
use virtio::VirtioDevice;
use zinnia::{
    alloc::sync::Arc,
    arch,
    device::{
        Device,
        block::{BioRequest, BlockDevice, BlockLimits, BlockOp},
    },
    log,
    memory::Register,
    posix::errno::{EResult, Errno},
    vfs::file::{FileOps, OpenFlags},
    warn,
};

const VIRTIO_BLK_MAJOR: u32 = 254;
const MAX_TRANSFER_BYTES: usize = 1024 * 1024;

pub struct Geometry {
    lba_size: usize,
    lba_count: u64,
    sectors_per_lba: u64,
    max_lbas: usize,
    max_segments: usize,
    read_only: bool,
}

impl Geometry {
    pub fn read(virtio: &VirtioDevice, features: u32, queue_size: u16) -> Result<Self, BlkError> {
        let read = |reg: Register<u32>| {
            virtio
                .read_config32(reg)
                .map_err(|_| BlkError::UnsupportedLayout)
        };

        let capacity = (read(spec::config::CAPACITY_HI)? as u64) << 32
            | read(spec::config::CAPACITY_LO)? as u64;

        let lba_size = if features & spec::VIRTIO_BLK_F_BLK_SIZE != 0 {
            let blk_size = read(spec::config::BLK_SIZE)? as usize;
            if blk_size.is_power_of_two()
                && blk_size >= spec::SECTOR_SIZE
                && blk_size <= arch::virt::get_page_size()
            {
                blk_size
            } else {
                warn!("Ignoring unusable block size {blk_size}");
                spec::SECTOR_SIZE
            }
        } else {
            spec::SECTOR_SIZE
        };

        let sectors_per_lba = (lba_size / spec::SECTOR_SIZE) as u64;
        let lba_count = capacity / sectors_per_lba;
        if lba_count == 0 {
            return Err(BlkError::UnsupportedLayout);
        }

        let hardware_segments = (queue_size as usize).saturating_sub(2).max(1);
        let max_segments = if features & spec::VIRTIO_BLK_F_SEG_MAX != 0 {
            (read(spec::config::SEG_MAX)? as usize).clamp(1, hardware_segments)
        } else {
            hardware_segments
        };

        let max_bytes = if features & spec::VIRTIO_BLK_F_SIZE_MAX != 0 {
            match read(spec::config::SIZE_MAX)? as usize {
                0 => MAX_TRANSFER_BYTES,
                size_max => size_max.min(MAX_TRANSFER_BYTES),
            }
        } else {
            MAX_TRANSFER_BYTES
        };

        Ok(Self {
            lba_size,
            lba_count,
            sectors_per_lba,
            max_lbas: (max_bytes / lba_size).max(1),
            max_segments,
            read_only: features & spec::VIRTIO_BLK_F_RO != 0,
        })
    }
}

pub struct VirtioBlkDevice {
    queue: Arc<RequestQueue>,
    geometry: Geometry,
    minor: u32,
}

impl VirtioBlkDevice {
    pub fn new(queue: Arc<RequestQueue>, geometry: Geometry, minor: u32) -> Self {
        log!(
            "New block device: {} byte blocks, {} MBs total, {}",
            geometry.lba_size,
            (geometry.lba_count * geometry.lba_size as u64) / 1024 / 1024,
            if geometry.read_only {
                "read-only"
            } else {
                "read-write"
            }
        );
        Self {
            queue,
            geometry,
            minor,
        }
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn get_lba_size(&self) -> usize {
        self.geometry.lba_size
    }

    fn lba_count(&self) -> u64 {
        self.geometry.lba_count
    }

    fn limits(&self) -> BlockLimits {
        BlockLimits {
            max_lbas: self.geometry.max_lbas,
            max_segments: self.geometry.max_segments,
        }
    }

    fn submit_bio(&self, bio: &Arc<BioRequest>) -> EResult<()> {
        if self.geometry.read_only && bio.op() == BlockOp::Write {
            bio.complete(Err(Errno::EROFS));
            return Ok(());
        }

        let Some(end_lba) = bio.lba().checked_add(bio.num_lbas() as u64) else {
            bio.complete(Err(Errno::EOVERFLOW));
            return Ok(());
        };
        if end_lba > self.geometry.lba_count {
            bio.complete(match bio.op() {
                BlockOp::Read => Ok(0),
                BlockOp::Write => Err(Errno::ENOSPC),
            });
            return Ok(());
        }

        let Some(sector) = bio.lba().checked_mul(self.geometry.sectors_per_lba) else {
            bio.complete(Err(Errno::EOVERFLOW));
            return Ok(());
        };

        let kind = match bio.op() {
            BlockOp::Read => spec::req_type::IN,
            BlockOp::Write => spec::req_type::OUT,
        };

        if let Err(error) = self.queue.submit(bio, kind, sector) {
            bio.complete(Err(error.into()));
            return Ok(());
        }

        self.queue.wait_if_polling(bio);
        Ok(())
    }
}

impl Device for VirtioBlkDevice {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(self.clone())
    }

    fn major(&self) -> u32 {
        VIRTIO_BLK_MAJOR
    }

    fn minor(&self) -> u32 {
        self.minor
    }
}
