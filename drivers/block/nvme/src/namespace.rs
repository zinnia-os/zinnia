use crate::{command::ReadWriteCommand, controller::Controller};
use zinnia::{
    alloc::sync::Arc,
    device::BlockDevice,
    log,
    memory::{PhysAddr, VirtAddr},
    posix::errno::{EResult, Errno},
    vfs::File,
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

    fn read_lba(&self, buffer: PhysAddr, num_lbas: usize, start_lba: u64) -> EResult<usize> {
        if start_lba + num_lbas as u64 > self.lba_count {
            return Ok(0);
        }

        let read_count = match *self.controller.mdts.lock() {
            Some(_) => num_lbas.min(16), // TODO: num_lbas.min((x >> self.lba_shift) as usize),
            None => num_lbas,
        };
        let mut ioq_guard = self.controller.io_queue.lock();
        let ioq = ioq_guard.as_mut().ok_or(Errno::EIO)?;

        ioq.submit_cmd(ReadWriteCommand {
            buffer,
            do_write: false,
            start_lba,
            num_lbas: read_count,
            bytes: read_count << self.lba_shift,
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

        Ok(read_count)
    }

    fn write_lba(&self, buffer: PhysAddr, sector_start: u64) -> EResult<()> {
        let mut ioq_guard = self.controller.io_queue.lock();
        let ioq = ioq_guard.as_mut().ok_or(Errno::EIO)?;

        ioq.submit_cmd(ReadWriteCommand {
            buffer,
            do_write: true,
            start_lba: sector_start,
            num_lbas: 1, // 1 LBA
            bytes: 512,
            control: 0,
            ds_mgmt: 0,
            ref_tag: 0,
            app_tag: 0,
            app_mask: 0,
            nsid: self.nsid,
        })
        .map_err(|_| Errno::ENXIO)?;

        if !ioq.next_completion().unwrap().status.is_success() {
            return Err(Errno::ENXIO);
        }

        Ok(())
    }

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        let _ = (file, request, arg);
        Err(Errno::EINVAL)
    }
}
