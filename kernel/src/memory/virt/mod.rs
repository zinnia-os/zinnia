pub mod fault;
pub mod mmu;

use super::{VirtAddr, pmm::AllocFlags};
use crate::{
    arch::{self},
    memory::{cache::MemoryObject, pmm::KernelAlloc, virt::mmu::PageTable},
    posix::errno::{EResult, Errno},
    uapi,
    util::{divide_up, once::Once},
};
use alloc::{collections::btree_set::BTreeSet, sync::Arc, vec::Vec};
use bitflags::bitflags;
use core::{
    fmt::Debug,
    num::NonZeroUsize,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};

// TODO: Kernel stacks should be mapped, not just on the HHDM. Otherwise we can't check for overflows.
pub const KERNEL_STACK_SIZE: usize = 0x8000;

bitflags! {
    /// PTE protection flags.
    #[derive(Debug, Copy, Clone)]
    pub struct PteFlags: u8 {
        /// Page can be read from.
        const Read = 1 << 0;
        /// Page can be written to.
        const Write = 1 << 1;
        /// Page has executable code.
        const Exec = 1 << 2;
        /// Page can be accessed by the user.
        const User = 1 << 3;
        /// Page is a large page.
        const Large = 1 << 4;
        /// Page is a directory to the next level.
        const Directory = 1 << 5;
    }

    /// Page protection flags.
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub struct VmFlags: u8 {
        /// Page can be read from.
        const Read = 1 << 0;
        /// Page can be written to.
        const Write = 1 << 1;
        /// Page has executable code.
        const Exec = 1 << 2;
        /// The page is shared between address spaces.
        const Shared = 1 << 3;
        /// This page is to be copied on write.
        const CopyOnWrite = 1 << 4;
    }
}

impl VmFlags {
    fn as_pte(self) -> PteFlags {
        let mut result = PteFlags::empty();
        if self.contains(VmFlags::Read) {
            result |= PteFlags::Read
        }
        if self.contains(VmFlags::Write) {
            result |= PteFlags::Write
        }
        if self.contains(VmFlags::Exec) {
            result |= PteFlags::Exec
        }
        result
    }
}

/// Page caching types.
pub enum VmCacheType {}

#[derive(Debug)]
pub enum PageTableError {
    PageTableEntryMissing,
    NeedAllocation,
    OutOfMemory,
}

pub(crate) static KERNEL_PAGE_TABLE: Once<Arc<PageTable>> = Once::new();

// TODO: Replace with allocator.
pub static KERNEL_MMAP_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);

pub struct AddressSpace {
    pub table: Arc<PageTable>,
    /// A map that translates global page offsets (virt / page_size) to a physical page and the flags of the mapping.
    pub mappings: BTreeSet<MappedObject>,
}

/// Represents a mapped object.
pub struct MappedObject {
    /// The starting virtual page number of the mapping.
    pub start_page: usize,
    /// The last virtual page number of the mapping.
    pub end_page: usize,
    /// The offset in the memory object.
    pub offset_page: usize,
    /// The mapped object.
    pub object: Arc<dyn MemoryObject>,
    /// A [`VmFlags`] object, but stored as an atomic value.
    flags: AtomicU8,
}

impl MappedObject {
    pub fn set_flags(&self, f: VmFlags) {
        self.flags.store(f.bits(), Ordering::SeqCst);
    }

    pub fn get_flags(&self) -> VmFlags {
        VmFlags::from_bits_truncate(self.flags.load(Ordering::SeqCst))
    }
}

impl Clone for MappedObject {
    fn clone(&self) -> Self {
        Self {
            start_page: self.start_page,
            end_page: self.end_page,
            offset_page: self.offset_page,
            object: self.object.clone(),
            flags: AtomicU8::new(self.flags.load(Ordering::SeqCst)),
        }
    }
}

impl PartialOrd for MappedObject {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.start_page.partial_cmp(&other.start_page)
    }
}

impl Ord for MappedObject {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.start_page.cmp(&other.start_page)
    }
}

impl PartialEq for MappedObject {
    fn eq(&self, other: &Self) -> bool {
        self.start_page == other.start_page
            && self.end_page == other.end_page
            && self.offset_page == other.offset_page
            && self.get_flags() == other.get_flags()
            && Arc::ptr_eq(&self.object, &other.object)
    }
}

impl Eq for MappedObject {}

impl AddressSpace {
    pub fn new() -> Self {
        Self {
            table: Arc::new(PageTable::new_user::<KernelAlloc>(AllocFlags::empty())),
            mappings: BTreeSet::new(),
        }
    }

    /// Maps an object into the address space.
    pub fn map_object(
        &mut self,
        object: Arc<dyn MemoryObject>,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        offset: uapi::off_t,
    ) -> EResult<()> {
        // `addr + len` may not overflow if the mapping is fixed.
        if addr.value().checked_add(len.into()).is_none() {
            return Err(Errno::ENOMEM);
        }

        let page_size = arch::virt::get_page_size();
        if addr.value() % page_size != offset as usize % page_size {
            return Err(Errno::EINVAL);
        }

        let start_page = addr.value() / page_size;
        let end_page = start_page + divide_up(len.into(), page_size);

        let overlapping = self
            .mappings
            .iter()
            .filter(|mapping| start_page < mapping.end_page && mapping.start_page < end_page)
            .cloned()
            .collect::<Vec<_>>();

        // Split any mappings that got shadowed.
        for mapping in overlapping.iter() {
            self.mappings.remove(mapping);
            // If new mapping completely shadows the old mapping.
            if start_page <= mapping.start_page && end_page >= mapping.end_page {
                for p in mapping.start_page..mapping.end_page {
                    let page_addr = (p * page_size).into();
                    _ = self.table.unmap_single::<KernelAlloc>(page_addr);
                }
            }
            // If new mapping partially shadows the old mapping.
            else {
                for p in start_page.max(mapping.start_page)..end_page.min(mapping.end_page) {
                    let page_addr = (p * page_size).into();
                    _ = self.table.unmap_single::<KernelAlloc>(page_addr);
                }

                let head_pages = start_page.saturating_sub(mapping.start_page);

                let tail_pages = mapping.end_page.saturating_sub(end_page);

                // Insert the leftmost pages.
                if head_pages > 0 {
                    self.mappings.insert(MappedObject {
                        start_page: mapping.start_page,
                        end_page: mapping.start_page + head_pages,
                        offset_page: mapping.offset_page,
                        object: mapping.object.clone(),
                        flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                    });
                }

                // Insert the rightmost pages.
                if tail_pages > 0 {
                    self.mappings.insert(MappedObject {
                        start_page: mapping.end_page - tail_pages,
                        end_page: mapping.end_page,
                        offset_page: mapping.offset_page + head_pages + (end_page - start_page),
                        object: mapping.object.clone(),
                        flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                    });
                }
            }
        }

        self.mappings.insert(MappedObject {
            start_page,
            end_page,
            offset_page: offset as usize / page_size,
            object: object.clone(),
            flags: AtomicU8::new(prot.bits()),
        });

        Ok(())
    }

    pub fn protect(&mut self, addr: VirtAddr, len: NonZeroUsize, prot: VmFlags) -> EResult<()> {
        // `addr + len` may not overflow if the mapping is fixed.
        if addr.value().checked_add(len.into()).is_none() {
            return Err(Errno::ENOMEM);
        }

        let page_size = arch::virt::get_page_size();
        if !addr.value().is_multiple_of(page_size) {
            return Err(Errno::EINVAL);
        }

        let start_page = addr.value() / page_size;
        let end_page = start_page + divide_up(len.into(), page_size);

        let overlapping = self
            .mappings
            .iter()
            .filter(|mapping| start_page < mapping.end_page && mapping.start_page < end_page)
            .cloned()
            .collect::<Vec<_>>();

        // Split any mappings that got shadowed.
        for mapping in overlapping {
            // If new mapping completely shadows the old mapping.
            if start_page <= mapping.start_page && end_page >= mapping.end_page {
                self.mappings.remove(&mapping);
                self.mappings.insert(MappedObject {
                    flags: AtomicU8::new(prot.bits()),
                    ..mapping
                });
                self.table
                    .remap_range::<KernelAlloc>(
                        (mapping.start_page * page_size).into(),
                        prot,
                        (mapping.end_page - mapping.start_page) * page_size,
                    )
                    .map_err(|_| Errno::ENOMEM)?;
            }
            // If new mapping partially shadows the old mapping.
            else {
                // TODO
                self.mappings.remove(&mapping);
                self.table
                    .unmap_range::<KernelAlloc>(
                        (start_page.max(mapping.start_page) * page_size).into(),
                        (end_page.min(mapping.end_page) - start_page) * page_size,
                    )
                    .map_err(|_| Errno::ENOMEM)?;

                let head_pages = start_page.saturating_sub(mapping.start_page);

                let tail_pages = mapping.end_page.saturating_sub(end_page);

                // Insert the leftmost pages.
                if head_pages > 0 {
                    self.mappings.insert(MappedObject {
                        start_page: mapping.start_page,
                        end_page: mapping.start_page + head_pages,
                        offset_page: mapping.offset_page,
                        object: mapping.object.clone(),
                        flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                    });
                }

                // Insert the rightmost pages.
                if tail_pages > 0 {
                    self.mappings.insert(MappedObject {
                        start_page: mapping.end_page - tail_pages,
                        end_page: mapping.end_page,
                        offset_page: mapping.offset_page + head_pages + (end_page - start_page),
                        object: mapping.object.clone(),
                        flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                    });
                }

                // Insert the new mapping.
                self.mappings.insert(MappedObject {
                    flags: AtomicU8::new(prot.bits()),
                    ..mapping
                });
            }
        }

        Ok(())
    }

    pub fn unmap(&self, addr: VirtAddr, len: NonZeroUsize) -> EResult<()> {
        let _ = (len, addr);
        // TODO
        Ok(())
    }

    /// Checks if the entire range is mapped in this address space.
    pub fn is_mapped(&self, addr: VirtAddr, len: usize) -> bool {
        let page_size = arch::virt::get_page_size();
        let num_pages = divide_up(len, page_size);
        let start_page = addr.value() / page_size;

        let mut prev = None;

        for mapping in self.mappings.iter().filter(|mapping| {
            start_page < mapping.end_page && mapping.start_page < start_page + num_pages
        }) {
            if let Some(e) = prev
                && e + 1 != mapping.start_page
            {
                return false;
            }

            prev = Some(mapping.end_page);
        }

        // If the filter didn't contain any matches, the range is completely outside of any
        // mapped memory. This would fall through and skip the gap check, so we need to check
        // if there was at least one iteration.
        prev.is_some()
    }

    pub fn clear(&mut self) {
        self.mappings.clear();
    }

    pub fn fork(&self) -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        let mut result = Self::new();

        // Copy over existing mappings, but make a copy of private mappings.
        for obj in self.mappings.iter() {
            if obj.get_flags().contains(VmFlags::Shared) {
                result.mappings.insert(obj.clone());
            } else {
                obj.set_flags(obj.get_flags() | VmFlags::CopyOnWrite);

                result.mappings.insert(obj.clone());

                // Map the object as read only in order to handle CoW.
                for p in obj.start_page..obj.end_page {
                    if self.table.is_mapped((p * page_size).into()) {
                        self.table
                            .remap_single::<KernelAlloc>((p * page_size).into(), VmFlags::Read)
                            .unwrap();
                    }
                }
            }
        }

        Ok(result)
    }
}

unsafe extern "C" {
    pub unsafe static LD_KERNEL_START: u8;
    pub unsafe static LD_TEXT_START: u8;
    pub unsafe static LD_TEXT_END: u8;
    pub unsafe static LD_RODATA_START: u8;
    pub unsafe static LD_RODATA_END: u8;
    pub unsafe static LD_DATA_START: u8;
    pub unsafe static LD_DATA_END: u8;
}
