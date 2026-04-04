use super::BlockDevice;
use crate::{
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr},
    posix::errno::{EResult, Errno},
};
use alloc::{fmt, string::String, sync::Arc, vec::Vec};
use core::slice;

const GPT_SIGNATURE: u64 = 0x5452415020494645; // "EFI PART"

/// A 128-bit GUID, stored in mixed-endian format as per GPT spec.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct Guid {
    pub time_low: u32,
    pub time_mid: u16,
    pub time_hi_version: u16,
    pub clock_seq: [u8; 2],
    pub node: [u8; 6],
}

impl Guid {
    pub const ZERO: Guid = Guid {
        time_low: 0,
        time_mid: 0,
        time_hi_version: 0,
        clock_seq: [0; 2],
        node: [0; 6],
    };

    pub fn is_zero(&self) -> bool {
        let tl = self.time_low;
        let tm = self.time_mid;
        let th = self.time_hi_version;
        tl == 0 && tm == 0 && th == 0 && self.clock_seq == [0; 2] && self.node == [0; 6]
    }

    pub fn to_string(&self) -> String {
        let time_low = self.time_low;
        let time_mid = self.time_mid;
        let time_hi = self.time_hi_version;
        let cs = self.clock_seq;
        let node = self.node;
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            time_low,
            time_mid,
            time_hi,
            cs[0],
            cs[1],
            node[0],
            node[1],
            node[2],
            node[3],
            node[4],
            node[5],
        )
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

/// GPT header at LBA 1.
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct GptHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    header_crc32: u32,
    reserved: u32,
    my_lba: u64,
    alternate_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: Guid,
    partition_entry_lba: u64,
    num_partition_entries: u32,
    partition_entry_size: u32,
    partition_entry_array_crc32: u32,
}

/// A single GPT partition entry (128 bytes minimum).
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct RawGptPartitionEntry {
    type_guid: Guid,
    unique_guid: Guid,
    start_lba: u64,
    end_lba: u64,
    attributes: u64,
    name: [u16; 36],
}

/// Parsed partition info.
#[derive(Debug, Clone)]
pub struct GptPartition {
    pub type_guid: Guid,
    pub unique_guid: Guid,
    pub start_lba: u64,
    pub end_lba: u64,
    pub attributes: u64,
}

/// Scans a block device for a GPT partition table.
/// Returns a list of non-empty partition entries.
pub fn scan_gpt(device: Arc<dyn BlockDevice>) -> EResult<Vec<GptPartition>> {
    let lba_size = device.get_lba_size();
    if lba_size == 0 {
        return Err(Errno::EINVAL);
    }

    // Allocate a buffer large enough for at least one LBA.
    let buf_size = lba_size.max(512);
    let buf_phys = KernelAlloc::alloc_bytes(buf_size, AllocFlags::empty())?;

    // Read LBA 1 (GPT header).
    let result = {
        device.read_lba(buf_phys, 1, 1)?;

        let header: GptHeader = unsafe {
            let ptr = buf_phys.as_hhdm::<u8>();
            core::ptr::read_unaligned(ptr as *const GptHeader)
        };

        if header.signature != GPT_SIGNATURE {
            return Err(Errno::ENODATA);
        }

        let num_entries = header.num_partition_entries as usize;
        let entry_size = header.partition_entry_size as usize;
        let entry_lba = header.partition_entry_lba;

        if entry_size < size_of::<RawGptPartitionEntry>() {
            return Err(Errno::EINVAL);
        }

        // Read all partition entries.
        let total_bytes = num_entries * entry_size;
        let total_lbas = total_bytes.div_ceil(lba_size);

        // Allocate a big enough buffer for all entries.
        let entry_buf_size = total_lbas * lba_size;
        let entry_buf = KernelAlloc::alloc_bytes(entry_buf_size, AllocFlags::empty())?;

        let read_result = {
            // Read entries in chunks.
            let mut lbas_read = 0;
            while lbas_read < total_lbas {
                let chunk = (total_lbas - lbas_read).min(64);
                let offset = PhysAddr::new(entry_buf.value() + lbas_read * lba_size);
                device.read_lba(offset, chunk, entry_lba + lbas_read as u64)?;
                lbas_read += chunk;
            }

            let entry_bytes: &[u8] =
                unsafe { slice::from_raw_parts(entry_buf.as_hhdm(), entry_buf_size) };

            let mut partitions = Vec::new();

            for i in 0..num_entries {
                let offset = i * entry_size;
                if offset + size_of::<RawGptPartitionEntry>() > entry_bytes.len() {
                    break;
                }

                let raw: RawGptPartitionEntry = unsafe {
                    core::ptr::read_unaligned(
                        entry_bytes.as_ptr().add(offset) as *const RawGptPartitionEntry
                    )
                };

                if raw.type_guid.is_zero() {
                    continue;
                }

                partitions.push(GptPartition {
                    type_guid: raw.type_guid,
                    unique_guid: raw.unique_guid,
                    start_lba: raw.start_lba,
                    end_lba: raw.end_lba,
                    attributes: raw.attributes,
                });
            }

            Ok(partitions)
        };

        unsafe { KernelAlloc::dealloc_bytes(entry_buf, entry_buf_size) };
        read_result
    };

    unsafe { KernelAlloc::dealloc_bytes(buf_phys, buf_size) };
    result
}
