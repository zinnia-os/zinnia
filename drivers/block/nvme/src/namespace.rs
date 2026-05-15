use crate::{command::ReadWriteCommand, controller::Controller};
use zinnia::{
    alloc::sync::Arc,
    device::{
        Device,
        block::{BlockCompletion, BlockDevice, BlockIo, BlockOp},
    },
    log,
    posix::errno::{EResult, Errno},
    vfs::file::{FileOps, OpenFlags},
};

pub struct Namespace {
    controller: Arc<Controller>,
    nsid: u32,
    lba_shift: u8,
    lba_count: u64,
}

impl Namespace {
    pub fn new(controller: Arc<Controller>, nsid: u32, lba_shift: u8, lba_count: u64) -> Self {
        log!(
            "New namespace: ID {nsid}, LBA size {} bytes, {} MBs total",
            1 << lba_shift,
            (lba_count << lba_shift) / 1024 / 1024
        );
        Self {
            controller,
            nsid,
            lba_shift,
            lba_count,
        }
    }

    pub fn get_id(&self) -> u32 {
        self.nsid
    }
}

impl BlockDevice for Namespace {
    fn get_lba_size(&self) -> usize {
        1 << self.lba_shift
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

        let lbas_per_page = zinnia::arch::virt::get_page_size() >> self.lba_shift;
        let transfer_lbas = match *self.controller.mdts.lock() {
            Some(mdts) => io.num_lbas().min((mdts >> self.lba_shift).max(1)),
            None => io.num_lbas(),
        }
        .min(lbas_per_page.max(1));

        let mut ioq_guard = self.controller.io_queue.lock();
        let ioq = ioq_guard.as_mut().ok_or(Errno::EIO)?;
        let segment = io.first_segment();

        ioq.submit_cmd(ReadWriteCommand {
            buffer: segment.phys(),
            do_write: io.op() == BlockOp::Write,
            start_lba: io.lba(),
            num_lbas: transfer_lbas,
            bytes: transfer_lbas << self.lba_shift,
            control: 0,
            ds_mgmt: 0,
            ref_tag: 0,
            app_tag: 0,
            app_mask: 0,
            nsid: self.nsid,
        })
        .map_err(|_| Errno::ENXIO)?;

        let comp = ioq.next_completion().unwrap();
        if !comp.status.is_success() {
            return Err(Errno::EFAULT);
        }

        Ok(BlockCompletion {
            lbas: transfer_lbas,
        })
    }
}

impl Device for Namespace {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(self.clone())
    }

    fn major(&self) -> u32 {
        159
    }

    fn minor(&self) -> u32 {
        self.nsid
    }
}
