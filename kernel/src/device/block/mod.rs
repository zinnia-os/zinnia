pub mod bio;
pub mod gpt;
pub mod io;
pub mod partition;
pub mod ram;

use crate::device::Device;
use crate::{
    arch::virt::get_page_size,
    memory::{IovecIter, VirtAddr},
    posix::errno::{EResult, Errno},
    process::Identity,
    vfs::{self, File, file::FileOps, fs::devtmpfs, inode::Mode},
};
use alloc::{format, sync::Arc, vec::Vec};

pub use bio::{BioRequest, BlockLimits};
pub use io::{BlockBuffer, BlockOp, BlockSegment};

const MAX_RAW_CHUNK: usize = 256 * 1024;

pub trait BlockDevice: Device {
    /// Gets the size of a sector in bytes.
    fn get_lba_size(&self) -> usize;

    /// Returns the total number of LBAs on this device.
    fn lba_count(&self) -> u64;

    fn limits(&self) -> BlockLimits;

    fn submit_bio(&self, bio: &Arc<BioRequest>) -> EResult<()>;

    fn handle_ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        let _ = (file, request, arg);
        Err(Errno::ENOTTY)
    }
}

pub fn submit_all(
    dev: &dyn BlockDevice,
    op: BlockOp,
    lba: u64,
    num_lbas: usize,
    segments: &[BlockSegment],
) -> EResult<usize> {
    if num_lbas == 0 {
        return Ok(0);
    }

    let lba_size = dev.get_lba_size();
    if lba_size == 0 {
        return Err(Errno::EINVAL);
    }

    let dev_lbas = dev.lba_count();
    let num_lbas = if op == BlockOp::Read {
        if lba >= dev_lbas {
            return Ok(0);
        }
        num_lbas.min((dev_lbas - lba) as usize)
    } else {
        num_lbas
    };
    let total_bytes = num_lbas * lba_size;

    let limits = dev.limits();
    let page_size = get_page_size();
    let lbas_per_page = (page_size / lba_size).max(1);
    let mut max_lbas = limits.max_lbas.max(1);
    if max_lbas > lbas_per_page {
        max_lbas -= max_lbas % lbas_per_page;
    }
    let max_segments = limits.max_segments.max(1);

    let mut children: Vec<Arc<BioRequest>> = Vec::new();
    let mut cur: Vec<BlockSegment> = Vec::new();
    let mut cur_lbas = 0usize;
    let mut cur_lba = lba;
    let mut next_lba = lba;
    let mut consumed = 0usize;

    let mut submit = |segs: Vec<BlockSegment>, start: u64, lbas: usize| -> EResult<()> {
        let child = BioRequest::new(op, start, lbas, lba_size, segs)?;
        if let Err(e) = dev.submit_bio(&child) {
            child.complete(Err(e));
        }
        children.push(child);
        Ok(())
    };

    'outer: for seg in segments {
        let mut phys = seg.phys();
        let mut remaining = seg.len();
        while remaining > 0 {
            if consumed >= total_bytes {
                break 'outer;
            }
            if cur_lbas >= max_lbas || cur.len() >= max_segments {
                submit(core::mem::take(&mut cur), cur_lba, cur_lbas)?;
                cur_lba = next_lba;
                cur_lbas = 0;
            }
            let room_bytes = (max_lbas - cur_lbas) * lba_size;
            let take = remaining.min(room_bytes).min(total_bytes - consumed);
            cur.push(BlockSegment::new(phys, take));
            let take_lbas = take / lba_size;
            cur_lbas += take_lbas;
            next_lba += take_lbas as u64;
            consumed += take;
            phys = phys + take;
            remaining -= take;
        }
    }
    if !cur.is_empty() {
        submit(core::mem::take(&mut cur), cur_lba, cur_lbas)?;
    }

    let mut total = 0usize;
    let mut stop = false;
    let mut first_err = None;
    for child in &children {
        let expected = child.num_lbas();
        let result = child.wait();
        if stop {
            continue;
        }
        match result {
            Ok(n) => {
                total += n;
                if n < expected {
                    stop = true;
                }
            }
            Err(e) => {
                first_err = Some(e);
                stop = true;
            }
        }
    }

    if total == 0 {
        if let Some(e) = first_err {
            return Err(e);
        }
    }

    Ok(total)
}

fn one_segment(buffer: &BlockBuffer, offset: usize, bytes: usize) -> EResult<[BlockSegment; 1]> {
    let end = offset.checked_add(bytes).ok_or(Errno::EOVERFLOW)?;
    if end > buffer.len() {
        return Err(Errno::EINVAL);
    }
    Ok([BlockSegment::new(buffer.phys() + offset, bytes)])
}

pub fn read_into(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    read_into_at(dev, buffer, 0, num_lba, lba)
}

pub fn read_into_at(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    let seg = one_segment(buffer, offset, num_lba * dev.get_lba_size())?;
    submit_all(dev, BlockOp::Read, lba, num_lba, &seg)
}

pub fn read_exact_into_at(
    dev: &dyn BlockDevice,
    buffer: &mut BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<()> {
    if read_into_at(dev, buffer, offset, num_lba, lba)? < num_lba {
        return Err(Errno::EIO);
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
    write_from_at(dev, buffer, 0, num_lba, lba)
}

pub fn write_from_at(
    dev: &dyn BlockDevice,
    buffer: &BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<usize> {
    let seg = one_segment(buffer, offset, num_lba * dev.get_lba_size())?;
    submit_all(dev, BlockOp::Write, lba, num_lba, &seg)
}

pub fn write_all_from_at(
    dev: &dyn BlockDevice,
    buffer: &BlockBuffer,
    offset: usize,
    num_lba: usize,
    lba: u64,
) -> EResult<()> {
    if write_from_at(dev, buffer, offset, num_lba, lba)? < num_lba {
        return Err(Errno::EIO);
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

#[task(
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
    crate::device::register_block_node(
        format!("block/{}", name).as_bytes(),
        device.clone(),
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

        crate::device::register_block_node(
            format!("block/{}", part_name).as_bytes(),
            part_dev,
            Mode::from_bits_truncate(0o660),
        )?;

        let root = devtmpfs::get_root();
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
        let want = buffer.len() as u64;

        let cap_bytes = (want + lba_size_u64)
            .min(MAX_RAW_CHUNK as u64 + lba_size_u64)
            .div_ceil(lba_size_u64)
            * lba_size_u64;
        let mut tmp = BlockBuffer::new(cap_bytes as usize)?;
        let mut progress = 0u64;

        'a: loop {
            if progress >= want {
                break 'a Ok(progress as isize);
            }

            let abs = progress + offset;
            let misalign = abs % lba_size_u64;
            let start_lba = abs / lba_size_u64;
            let remaining = want - progress;
            let chunk_bytes = (misalign + remaining).min(cap_bytes);
            let chunk_lbas = chunk_bytes.div_ceil(lba_size_u64).max(1);

            let read_lbas = match read_into(self, &mut tmp, chunk_lbas as usize, start_lba) {
                Ok(0) => break 'a Ok(progress as isize),
                Ok(n) => n as u64,
                Err(e) if progress == 0 => break 'a Err(e),
                Err(_) => break 'a Ok(progress as isize),
            };

            let chunk_slice = &tmp.as_slice()[..(read_lbas * lba_size_u64) as usize];
            let start = misalign as usize;
            if start >= chunk_slice.len() {
                break 'a Ok(progress as isize);
            }
            let copy_len = (chunk_slice.len() - start).min(remaining as usize);
            if copy_len == 0 {
                break 'a Ok(progress as isize);
            }

            buffer.set_offset(progress as _);
            if let Err(err) = buffer.copy_from_slice(&chunk_slice[start..][..copy_len]) {
                break 'a Err(err);
            }
            progress += copy_len as u64;
        }
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let lba_size = self.get_lba_size();
        if lba_size == 0 {
            return Err(Errno::EINVAL);
        }
        let lba_size_u64 = lba_size as u64;
        let want = buffer.len() as u64;

        let cap_bytes = want.min(MAX_RAW_CHUNK as u64).div_ceil(lba_size_u64).max(1) * lba_size_u64;
        let mut tmp = BlockBuffer::new(cap_bytes as usize)?;
        let mut progress = 0u64;

        'a: loop {
            if progress >= want {
                break 'a Ok(progress as isize);
            }

            let abs = progress + offset;
            let misalign = abs % lba_size_u64;
            let start_lba = abs / lba_size_u64;
            let remaining = want - progress;

            if misalign != 0 || remaining < lba_size_u64 {
                let copy = (lba_size_u64 - misalign).min(remaining);
                if read_into(self, &mut tmp, 1, start_lba).is_err() && progress == 0 {
                    break 'a Err(Errno::EIO);
                }
                buffer.set_offset(progress as _);
                if let Err(err) = buffer
                    .copy_to_slice(&mut tmp.as_mut_slice()[misalign as usize..][..copy as usize])
                {
                    break 'a Err(err);
                }
                match write_from(self, &tmp, 1, start_lba) {
                    Ok(0) | Err(_) if progress == 0 => break 'a Err(Errno::EIO),
                    Ok(0) | Err(_) => break 'a Ok(progress as isize),
                    Ok(_) => {}
                }
                progress += copy;
                continue;
            }

            let chunk_lbas = (remaining / lba_size_u64).min(cap_bytes / lba_size_u64);
            let chunk_bytes = chunk_lbas * lba_size_u64;
            buffer.set_offset(progress as _);
            if let Err(err) = buffer.copy_to_slice(&mut tmp.as_mut_slice()[..chunk_bytes as usize])
            {
                break 'a Err(err);
            }
            match write_from(self, &tmp, chunk_lbas as usize, start_lba) {
                Ok(0) if progress == 0 => break 'a Err(Errno::EIO),
                Ok(0) => break 'a Ok(progress as isize),
                Ok(n) => progress += n as u64 * lba_size_u64,
                Err(e) if progress == 0 => break 'a Err(e),
                Err(_) => break 'a Ok(progress as isize),
            }
        }
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.handle_ioctl(file, request, arg)
    }
}
