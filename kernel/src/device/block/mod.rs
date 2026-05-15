pub mod gpt;
pub mod io;
pub mod partition;

use crate::device::Device;
use crate::{
    memory::{IovecIter, VirtAddr},
    posix::errno::{EResult, Errno},
    process::Identity,
    vfs::{
        self, File,
        file::{FileOps, PollFlags},
        fs::devtmpfs,
        inode::{MknodTarget, Mode},
    },
};
use alloc::{format, sync::Arc};

pub use io::{BlockBuffer, BlockCompletion, BlockIo, BlockIter, BlockOp, BlockSegment};

pub trait BlockDevice: Device {
    /// Gets the size of a sector in bytes.
    fn get_lba_size(&self) -> usize;

    /// Returns the total number of LBAs on this device.
    fn lba_count(&self) -> u64;

    /// Submits a synchronous block I/O request.
    fn submit_io(&self, io: &mut BlockIo) -> EResult<BlockCompletion>;

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        let _ = (file, request, arg);
        Err(Errno::ENOTTY)
    }
}

pub fn read_into(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    let mut io = BlockIo::read(buffer, lba, num_lba, dev.get_lba_size())?;
    dev.submit_io(&mut io).map(|completion| completion.lbas)
}

pub fn read_into_at(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    let mut io = BlockIo::read_at(buffer, offset, lba, num_lba, dev.get_lba_size())?;
    dev.submit_io(&mut io).map(|completion| completion.lbas)
}

pub fn read_exact_into_at(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<()> {
    let lba_size = dev.get_lba_size();
    let mut done = 0;

    while done < num_lba {
        let read = read_into_at(
            dev,
            buffer,
            offset + done * lba_size,
            num_lba - done,
            lba + done as u64,
        )?;
        if read == 0 {
            return Err(Errno::EIO);
        }
        done += read;
    }

    Ok(())
}

pub fn read_exact_into(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    num_lba: usize,
    lba: u64,
) -> EResult<()> {
    read_exact_into_at(dev, buffer, 0, num_lba, lba)
}

pub fn write_from(
    dev: &dyn BlockDevice,
    buffer: &BlockBuffer,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    let mut io = BlockIo::write(buffer, lba, num_lba, dev.get_lba_size())?;
    dev.submit_io(&mut io).map(|completion| completion.lbas)
}

pub fn write_from_at(
    dev: &dyn BlockDevice,
    buffer: &BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    let mut io = BlockIo::write_at(buffer, offset, lba, num_lba, dev.get_lba_size())?;
    dev.submit_io(&mut io).map(|completion| completion.lbas)
}

pub fn write_all_from_at(
    dev: &dyn BlockDevice,
    buffer: &BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<()> {
    let lba_size = dev.get_lba_size();
    let mut done = 0;

    while done < num_lba {
        let written = write_from_at(
            dev,
            buffer,
            offset + done * lba_size,
            num_lba - done,
            lba + done as u64,
        )?;
        if written == 0 {
            return Err(Errno::EIO);
        }
        done += written;
    }

    Ok(())
}

pub fn write_all_from(
    dev: &dyn BlockDevice,
    buffer: &BlockBuffer,
    num_lba: usize,
    lba: u64,
) -> EResult<()> {
    write_all_from_at(dev, buffer, 0, num_lba, lba)
}

#[initgraph::task(
    name = "generic.device.block",
    depends = [devtmpfs::DEVTMPFS_STAGE]
)]
pub fn BLOCK_STAGE() {
    let root = devtmpfs::get_root();

    vfs::mkdir(
        root.clone(),
        root,
        b"block",
        Mode::from_bits_truncate(0o755),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/block");
}

/// Registers a block device by name and scans for partitions.
pub fn register_block_device(name: &str, device: Arc<dyn BlockDevice>) -> EResult<()> {
    // Register in devtmpfs as well.
    let root = devtmpfs::get_root();

    vfs::mknod(
        root.clone(),
        root,
        format!("block/{}", name).as_bytes(),
        Mode::from_bits_truncate(0o660),
        Some(MknodTarget::BlockDevice(device.clone())),
        &Identity::get_kernel(),
    )?;

    log!("Registered block device: \"{}\"", name);

    // Scan for GPT partitions.
    scan_partitions(name, device)?;

    Ok(())
}

/// Scans a block device for GPT partitions and registers each as a sub-device.
fn scan_partitions(parent_name: &str, device: Arc<dyn BlockDevice>) -> EResult<()> {
    let partitions = match gpt::scan_gpt(device.clone()) {
        Ok(p) => p,
        Err(_) => return Ok(()), // No GPT found, that's fine.
    };

    for (i, part) in partitions.iter().enumerate() {
        let part_name = format!("{}p{}", parent_name, i + 1);
        let part_dev = Arc::new(partition::PartitionDevice::new(
            device.clone(),
            part.start_lba,
            part.end_lba - part.start_lba + 1,
        ));

        let root = devtmpfs::get_root();

        vfs::mknod(
            root.clone(),
            root.clone(),
            format!("block/{}", part_name).as_bytes(),
            Mode::from_bits_truncate(0o660),
            Some(MknodTarget::BlockDevice(part_dev)),
            &Identity::get_kernel(),
        )?;

        let uuid_str = part.unique_guid.to_string();
        let type_str = part.type_guid.to_string();

        // TODO: This could conflict with other partitions.
        vfs::symlink(
            root.clone(),
            root.clone(),
            format!("block/parttype-{}", type_str).as_bytes(),
            part_name.as_bytes(),
            &Identity::get_kernel(),
        )?;
        vfs::symlink(
            root.clone(),
            root.clone(),
            format!("block/partuuid-{}", uuid_str).as_bytes(),
            part_name.as_bytes(),
            &Identity::get_kernel(),
        )?;

        log!(
            "Partition {}: \"{}\" Type: {} UUID: {}",
            i + 1,
            part_name,
            type_str,
            uuid_str
        );
    }

    Ok(())
}

impl<T: BlockDevice> FileOps for T {
    fn read(&self, _: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let lba_size = self.get_lba_size();
        if lba_size == 0 {
            return Err(Errno::EINVAL);
        }

        let lba_size_u64 = lba_size as u64;
        let mut max_lbas_per_iter = (buffer.len() as u64).div_ceil(lba_size_u64).max(1);
        max_lbas_per_iter = max_lbas_per_iter.saturating_add(1);

        let tmp_bytes_u64 = max_lbas_per_iter
            .checked_mul(lba_size_u64)
            .ok_or(Errno::ENOMEM)?;
        let tmp_bytes = usize::try_from(tmp_bytes_u64).map_err(|_| Errno::ENOMEM)?;

        let mut tmp = BlockBuffer::new(tmp_bytes)?;
        let mut progress = 0;

        let result = 'a: loop {
            if progress >= buffer.len() as u64 {
                break 'a Ok(progress as isize);
            }

            let misalign = (progress + offset) % lba_size_u64;
            let page_index = (progress + offset) / lba_size_u64;
            let remaining = buffer.len() as u64 - progress;
            let mut chunk_lbas = (misalign + remaining).div_ceil(lba_size_u64).max(1);
            chunk_lbas = chunk_lbas.min(max_lbas_per_iter);

            let read_lbas = match read_into(self, &mut tmp, chunk_lbas as usize, page_index) {
                Ok(0) => break 'a Ok(progress as isize),
                Ok(n) => n as u64,
                Err(e) if progress == 0 => break 'a Err(e),
                Err(_) => break 'a Ok(progress as isize),
            };

            let chunk_bytes = read_lbas * lba_size_u64;
            let chunk_slice = &tmp.as_slice()[..chunk_bytes as usize];

            let start = misalign as usize;
            if start >= chunk_slice.len() {
                break 'a Ok(progress as isize);
            }

            let mut copy_len = chunk_slice.len() - start;
            copy_len = copy_len.min(remaining as usize);
            if copy_len == 0 {
                break 'a Ok(progress as isize);
            }

            buffer.set_offset(progress as _);
            if let Err(err) = buffer.copy_from_slice(&chunk_slice[start..][..copy_len]) {
                break 'a Err(err);
            }
            progress += copy_len as u64;
        };

        result
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let sector_size = self.get_lba_size() as u64;
        if sector_size == 0 {
            return Err(Errno::EINVAL);
        }

        let mut tmp = BlockBuffer::new(sector_size as _)?;
        let mut progress = 0;

        let result = 'a: loop {
            if progress >= buffer.len() as u64 {
                break 'a Ok(progress as isize);
            }
            let misalign = (progress + offset) % sector_size;
            let page_index = (progress + offset) / sector_size;
            let copy_size = (sector_size - misalign).min(buffer.len() as u64 - progress);

            // Read the current LBA data.
            if let Err(e) = read_into(self, &mut tmp, 1, page_index) {
                if progress == 0 {
                    break 'a Err(e);
                } else {
                    break 'a Ok(progress as isize);
                }
            }

            {
                let page_slice = tmp.as_mut_slice();
                buffer.set_offset(progress as _);
                if let Err(err) =
                    buffer.copy_to_slice(&mut page_slice[misalign as usize..][..copy_size as usize])
                {
                    break 'a Err(err);
                }
            }

            // Write the new LBA data.
            if let Err(e) = write_from(self, &tmp, 1, page_index) {
                if progress == 0 {
                    break 'a Err(e);
                } else {
                    break 'a Ok(progress as isize);
                }
            }

            progress += copy_size;
        };

        result
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.handle_ioctl(file, request, arg)
    }

    fn poll(&self, file: &File, mask: PollFlags) -> EResult<PollFlags> {
        _ = (file, mask);
        Ok(mask)
    }
}
