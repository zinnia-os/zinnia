pub mod gpt;
pub mod partition;

use crate::{
    memory::{AllocFlags, IovecIter, KernelAlloc, PageAllocator, PhysAddr, VirtAddr},
    posix::errno::{EResult, Errno},
    vfs::{
        File,
        file::FileOps,
        fs::devtmpfs,
        inode::{Device, Mode},
    },
};
use alloc::{format, sync::Arc};
use core::slice;

pub trait BlockDevice: FileOps {
    /// Gets the size of a sector in bytes.
    fn get_lba_size(&self) -> usize;

    /// Returns the total number of LBAs on this device.
    fn lba_count(&self) -> u64 {
        0
    }

    /// Reads a logical block from from `lba` into the buffer.
    fn read_lba(&self, buffer: PhysAddr, num_lba: usize, lba: u64) -> EResult<usize>;

    /// Writes the buffer to the device starting at `sector_start`.
    fn write_lba(&self, buffer: PhysAddr, lba: u64) -> EResult<()>;

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize>;
}

/// Registers a block device by name and scans for partitions.
pub fn register_block_device(name: &str, device: Arc<dyn BlockDevice>) -> EResult<()> {
    // Register in devtmpfs as well.
    devtmpfs::register_device(
        name.as_bytes(),
        Device::BlockDevice(device.clone()),
        Mode::from_bits_truncate(0o660),
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

        devtmpfs::register_device(
            part_name.as_bytes(),
            Device::BlockDevice(part_dev),
            Mode::from_bits_truncate(0o660),
        )?;

        let uuid_str = part.unique_guid.to_string();
        devtmpfs::register_symlink(
            format!("partuuid-{}", uuid_str).as_bytes(),
            part_name.as_bytes(),
        )?;

        log!("Partition {}: \"{}\" UUID: {}", i + 1, part_name, uuid_str);
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

        let tmp_phys = KernelAlloc::alloc_bytes(tmp_bytes, AllocFlags::empty())?;
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

            let read_lbas = match self.read_lba(tmp_phys, chunk_lbas as usize, page_index) {
                Ok(0) => break 'a Ok(progress as isize),
                Ok(n) => n as u64,
                Err(e) if progress == 0 => return Err(e),
                Err(_) => break 'a Ok(progress as isize),
            };

            let chunk_bytes = read_lbas * lba_size_u64;
            let chunk_slice =
                unsafe { slice::from_raw_parts(tmp_phys.as_hhdm(), chunk_bytes as usize) };

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
            buffer.copy_from_slice(&chunk_slice[start..][..copy_len])?;
            progress += copy_len as u64;
        };

        unsafe { KernelAlloc::dealloc_bytes(tmp_phys, tmp_bytes) };

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

        let tmp_phys = KernelAlloc::alloc_bytes(sector_size as _, AllocFlags::empty())?;
        let mut progress = 0;

        let result = 'a: loop {
            if progress >= buffer.len() as u64 {
                break 'a Ok(progress as isize);
            }
            let misalign = (progress + offset) % sector_size;
            let page_index = (progress + offset) / sector_size;
            let copy_size = (sector_size - misalign).min(buffer.len() as u64 - progress);

            // Read the current LBA data.
            if let Err(e) = self.read_lba(tmp_phys, 1, page_index) {
                if progress == 0 {
                    return Err(e);
                } else {
                    break 'a Ok(progress as isize);
                }
            }

            {
                let page_slice: &mut [u8] =
                    unsafe { slice::from_raw_parts_mut(tmp_phys.as_hhdm(), sector_size as _) };
                buffer.set_offset(progress as _);
                buffer.copy_to_slice(&mut page_slice[misalign as usize..][..copy_size as usize])?;
            }

            // Write the new LBA data.
            if let Err(e) = self.write_lba(tmp_phys, page_index) {
                if progress == 0 {
                    return Err(e);
                } else {
                    break 'a Ok(progress as isize);
                }
            }

            progress += copy_size;
        };

        unsafe { KernelAlloc::dealloc_bytes(tmp_phys, sector_size as usize) };

        result
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.handle_ioctl(file, request, arg)
    }

    fn poll(&self, file: &File, mask: i16) -> EResult<i16> {
        _ = (file, mask);
        Ok(mask)
    }
}
