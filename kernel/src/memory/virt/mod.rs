pub mod allocator;
pub mod fault;
pub mod mmu;

use self::allocator::VirtualAllocator;
use super::{VirtAddr, pmm::AllocFlags};
use crate::{
    arch::{self},
    memory::{cache::MemoryObject, pmm::KernelAlloc, virt::mmu::PageTable},
    posix::errno::{EResult, Errno},
    uapi,
    util::{divide_up, mutex::spin::SpinMutex, once::Once},
};
use alloc::{collections::btree_set::BTreeSet, sync::Arc, vec::Vec};
use bitflags::bitflags;
use core::{
    fmt::Debug,
    num::NonZeroUsize,
    sync::atomic::{AtomicU8, Ordering},
};

const USER_MMAP_BASE: usize = 0x1_0000_0000;

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

/// Page caching types. MMIO device memory uses [`VmCacheType::Uncacheable`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum VmCacheType {
    /// Write-back caching. The default for ordinary RAM.
    #[default]
    Normal,
    /// Write-through caching.
    WriteThrough,
    /// Write-combining. Used for framebuffers/VRAM.
    WriteCombine,
    /// Uncacheable. Used for MMIO device memory.
    Uncacheable,
}

#[derive(Debug)]
pub enum PageTableError {
    PageTableEntryMissing,
    NeedAllocation,
    OutOfMemory,
}

pub(crate) static KERNEL_PAGE_TABLE: Once<Arc<PageTable>> = Once::new();
pub(crate) static KERNEL_VIRTUAL_ALLOCATOR: Once<SpinMutex<VirtualAllocator>> = Once::new();

pub struct AddressSpace {
    pub table: Arc<PageTable>,
    allocator: VirtualAllocator,
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
    /// The caching mode the object's pages are mapped with.
    cache: VmCacheType,
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
            cache: self.cache,
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
            allocator: Self::new_user_allocator(),
            mappings: BTreeSet::new(),
        }
    }

    pub fn new_kernel(table: Arc<PageTable>) -> Self {
        Self {
            table,
            allocator: Self::new_user_allocator(),
            mappings: BTreeSet::new(),
        }
    }

    fn new_user_allocator() -> VirtualAllocator {
        let page_size = arch::virt::get_page_size();
        let user_end = 1usize << (arch::virt::get_highest_bit_shift() - 1);

        VirtualAllocator::new(page_size.into(), user_end.into()).unwrap()
    }

    pub fn find_mmap_addr(&self, len: NonZeroUsize) -> EResult<VirtAddr> {
        self.allocator.find_free_from(USER_MMAP_BASE.into(), len)
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

        self.allocator.release(addr, len)?;
        self.allocator.reserve(addr, len)?;

        // Split any mappings that got shadowed.
        while let Some(mapping) = self.find_overlapping(start_page, end_page) {
            self.mappings.remove(&mapping);
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

                let overlap_end = end_page.min(mapping.end_page);
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
                        cache: mapping.cache,
                    });
                }

                // Insert the rightmost pages.
                if tail_pages > 0 {
                    self.mappings.insert(MappedObject {
                        start_page: mapping.end_page - tail_pages,
                        end_page: mapping.end_page,
                        offset_page: mapping.offset_page + (overlap_end - mapping.start_page),
                        object: mapping.object.clone(),
                        flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                        cache: mapping.cache,
                    });
                }
            }
        }

        let cache = object.cache_type();
        self.mappings.insert(MappedObject {
            start_page,
            end_page,
            offset_page: offset as usize / page_size,
            object: object.clone(),
            flags: AtomicU8::new(prot.bits()),
            cache,
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

        // Split any mappings that got shadowed.
        let mut cursor = start_page;
        while let Some(mapping) = self.find_overlapping(cursor, end_page) {
            let new_flags = prot | (mapping.get_flags() & (VmFlags::Shared | VmFlags::CopyOnWrite));
            let pte_flags = if new_flags.contains(VmFlags::CopyOnWrite) {
                new_flags & !VmFlags::Write
            } else {
                new_flags
            };
            // If new mapping completely shadows the old mapping.
            if start_page <= mapping.start_page && end_page >= mapping.end_page {
                self.mappings.remove(&mapping);
                let mapping_end = mapping.end_page;
                for p in mapping.start_page..mapping.end_page {
                    if self.table.is_mapped((p * page_size).into()) {
                        self.table
                            .remap_single::<KernelAlloc>((p * page_size).into(), pte_flags)
                            .map_err(|_| Errno::ENOMEM)?;
                    }
                }
                self.mappings.insert(MappedObject {
                    flags: AtomicU8::new(new_flags.bits()),
                    ..mapping
                });
                cursor = mapping_end;
            }
            // If new mapping partially shadows the old mapping.
            else {
                self.mappings.remove(&mapping);

                let overlap_start = start_page.max(mapping.start_page);
                let overlap_end = end_page.min(mapping.end_page);
                cursor = overlap_end;
                for p in overlap_start..overlap_end {
                    if self.table.is_mapped((p * page_size).into()) {
                        self.table
                            .remap_single::<KernelAlloc>((p * page_size).into(), pte_flags)
                            .map_err(|_| Errno::ENOMEM)?;
                    }
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
                        cache: mapping.cache,
                    });
                }

                // Insert the rightmost pages.
                if tail_pages > 0 {
                    self.mappings.insert(MappedObject {
                        start_page: mapping.end_page - tail_pages,
                        end_page: mapping.end_page,
                        offset_page: mapping.offset_page + (overlap_end - mapping.start_page),
                        object: mapping.object.clone(),
                        flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                        cache: mapping.cache,
                    });
                }

                // Insert the new mapping.
                self.mappings.insert(MappedObject {
                    start_page: overlap_start,
                    end_page: overlap_end,
                    offset_page: mapping.offset_page + (overlap_start - mapping.start_page),
                    object: mapping.object.clone(),
                    flags: AtomicU8::new(new_flags.bits()),
                    cache: mapping.cache,
                });
            }
        }

        Ok(())
    }

    pub fn unmap(&mut self, addr: VirtAddr, len: NonZeroUsize) -> EResult<()> {
        if addr.value().checked_add(len.into()).is_none() {
            return Err(Errno::ENOMEM);
        }

        let page_size = arch::virt::get_page_size();
        if !addr.value().is_multiple_of(page_size) {
            return Err(Errno::EINVAL);
        }

        self.harvest_dirty_range(addr, len)?;

        let start_page = addr.value() / page_size;
        let end_page = start_page + divide_up(len.into(), page_size);

        while let Some(mapping) = self.find_overlapping(start_page, end_page) {
            self.mappings.remove(&mapping);

            let overlap_start = start_page.max(mapping.start_page);
            let overlap_end = end_page.min(mapping.end_page);

            for p in overlap_start..overlap_end {
                let page_addr = (p * page_size).into();
                _ = self.table.unmap_single::<KernelAlloc>(page_addr);
            }

            if mapping.start_page < overlap_start {
                self.mappings.insert(MappedObject {
                    start_page: mapping.start_page,
                    end_page: overlap_start,
                    offset_page: mapping.offset_page,
                    object: mapping.object.clone(),
                    flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                    cache: mapping.cache,
                });
            }

            if overlap_end < mapping.end_page {
                self.mappings.insert(MappedObject {
                    start_page: overlap_end,
                    end_page: mapping.end_page,
                    offset_page: mapping.offset_page + (overlap_end - mapping.start_page),
                    object: mapping.object.clone(),
                    flags: AtomicU8::new(mapping.flags.load(Ordering::SeqCst)),
                    cache: mapping.cache,
                });
            }
        }

        self.allocator.release(addr, len)?;

        Ok(())
    }

    fn find_overlapping(&self, start_page: usize, end_page: usize) -> Option<MappedObject> {
        self.mappings
            .iter()
            .find(|mapping| start_page < mapping.end_page && mapping.start_page < end_page)
            .cloned()
    }

    pub fn harvest_dirty_range(&self, addr: VirtAddr, len: NonZeroUsize) -> EResult<()> {
        let end_addr = addr.value().checked_add(len.get()).ok_or(Errno::ENOMEM)?;
        let page_size = arch::virt::get_page_size();
        let start_page = addr.value() / page_size;
        let end_page = divide_up(end_addr, page_size);

        for mapping in self
            .mappings
            .iter()
            .filter(|mapping| start_page < mapping.end_page && mapping.start_page < end_page)
        {
            if !mapping.get_flags().contains(VmFlags::Shared) {
                continue;
            }

            let overlap_start = start_page.max(mapping.start_page);
            let overlap_end = end_page.min(mapping.end_page);

            for page in overlap_start..overlap_end {
                let page_addr = (page * page_size).into();
                if self.table.take_dirty(page_addr) {
                    let object_page = mapping.offset_page + (page - mapping.start_page);
                    mapping.object.mark_dirty_page(object_page);
                }
            }
        }

        Ok(())
    }

    pub fn sync_dirty_range(&self, addr: VirtAddr, len: NonZeroUsize) -> EResult<()> {
        self.harvest_dirty_range(addr, len)?;

        let end_addr = addr.value().checked_add(len.get()).ok_or(Errno::ENOMEM)?;
        let page_size = arch::virt::get_page_size();
        let start_page = addr.value() / page_size;
        let end_page = divide_up(end_addr, page_size);
        let mut objects = Vec::<Arc<dyn MemoryObject>>::new();

        for mapping in self
            .mappings
            .iter()
            .filter(|mapping| start_page < mapping.end_page && mapping.start_page < end_page)
        {
            if !mapping.get_flags().contains(VmFlags::Shared) {
                continue;
            }

            if !objects
                .iter()
                .any(|object| Arc::ptr_eq(object, &mapping.object))
            {
                objects.push(mapping.object.clone());
            }
        }

        for object in objects {
            object.sync()?;
        }

        Ok(())
    }

    pub fn harvest_dirty_object(&self, target: &Arc<dyn MemoryObject>) -> EResult<()> {
        let page_size = arch::virt::get_page_size();

        for mapping in self.mappings.iter() {
            if !mapping.get_flags().contains(VmFlags::Shared)
                || !Arc::ptr_eq(&mapping.object, target)
            {
                continue;
            }

            for page in mapping.start_page..mapping.end_page {
                let page_addr = (page * page_size).into();
                if self.table.take_dirty(page_addr) {
                    let object_page = mapping.offset_page + (page - mapping.start_page);
                    mapping.object.mark_dirty_page(object_page);
                }
            }
        }

        Ok(())
    }

    /// Checks if the entire range is mapped in this address space.
    pub fn is_mapped(&self, addr: VirtAddr, len: usize) -> bool {
        let page_size = arch::virt::get_page_size();
        let num_pages = divide_up(len, page_size);
        let start_page = addr.value() / page_size;
        let end_page = start_page + num_pages;

        let mut covered_until = start_page;

        for mapping in self
            .mappings
            .iter()
            .filter(|mapping| start_page < mapping.end_page && mapping.start_page < end_page)
        {
            if mapping.start_page > covered_until {
                return false;
            }

            covered_until = covered_until.max(mapping.end_page);
            if covered_until >= end_page {
                return true;
            }
        }

        false
    }

    pub fn clear(&mut self) {
        self.mappings.clear();
        self.allocator.clear();
    }

    pub fn fork(&self) -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        let mut result = Self {
            table: Arc::new(PageTable::new_user::<KernelAlloc>(AllocFlags::empty())),
            allocator: self.allocator.clone(),
            mappings: BTreeSet::new(),
        };

        // Copy over existing mappings, but make a copy of private mappings.
        for obj in self.mappings.iter() {
            if obj.get_flags().contains(VmFlags::Shared) {
                result.mappings.insert(obj.clone());
            } else {
                let old_flags = obj.get_flags();
                let cow_flags = old_flags | VmFlags::CopyOnWrite;
                obj.set_flags(cow_flags);

                result.mappings.insert(obj.clone());

                // Map the object as read only in order to handle CoW.
                if old_flags.contains(VmFlags::Write) {
                    for p in obj.start_page..obj.end_page {
                        if self.table.is_mapped((p * page_size).into()) {
                            self.table
                                .remap_single::<KernelAlloc>(
                                    (p * page_size).into(),
                                    cow_flags & !VmFlags::Write,
                                )
                                .unwrap();
                        }
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
