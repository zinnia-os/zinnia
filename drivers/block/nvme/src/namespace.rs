use crate::{command::ReadWriteCommand, controller::Controller, prp::build_prps};
use core::{hint::spin_loop, time::Duration};
use zinnia::{
    alloc::sync::Arc,
    clock,
    device::{
        Device,
        block::{BioRequest, BlockDevice, BlockLimits, BlockOp},
    },
    irq::lock::IrqLock,
    log,
    posix::errno::{EResult, Errno},
    vfs::file::{FileOps, OpenFlags},
};

const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Namespace {
    controller: Arc<Controller>,
    nsid: u32,
    lba_shift: u8,
    lba_count: u64,
    max_transfer_bytes: usize,
}

impl Namespace {
    pub fn new(
        controller: Arc<Controller>,
        nsid: u32,
        lba_shift: u8,
        lba_count: u64,
        max_transfer_bytes: usize,
    ) -> Self {
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
            max_transfer_bytes,
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

    fn limits(&self) -> BlockLimits {
        BlockLimits {
            max_lbas: (self.max_transfer_bytes >> self.lba_shift).max(1),
            max_segments: 512,
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

        let (prp1, prp2, prps) = match build_prps(bio.segments()) {
            Ok(x) => x,
            Err(_) => {
                bio.complete(Err(Errno::EIO));
                return Ok(());
            }
        };

        let queue = {
            let _irq = IrqLock::lock();
            self.controller.io_queue.lock().clone()
        };
        let Some(queue) = queue else {
            bio.complete(Err(Errno::EIO));
            return Ok(());
        };

        queue.submit(
            bio,
            prps,
            ReadWriteCommand {
                prp1,
                prp2,
                cid: 0,
                do_write: bio.op() == BlockOp::Write,
                start_lba: bio.lba(),
                num_lbas: bio.num_lbas(),
                bytes: bio.bytes(),
                control: 0,
                ds_mgmt: 0,
                ref_tag: 0,
                app_tag: 0,
                app_mask: 0,
                nsid: self.nsid,
            },
        );

        if queue.is_polling() {
            let deadline = clock::get_elapsed().saturating_add(COMPLETION_TIMEOUT);
            while !bio.is_done() {
                queue.drain();
                if bio.is_done() {
                    break;
                }
                if clock::get_elapsed() >= deadline {
                    bio.complete(Err(Errno::EIO));
                    break;
                }
                spin_loop();
            }
        }

        Ok(())
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
