use super::internal;
use crate::{
    memory::{VirtAddr, virt::mmu::PageTable},
    sched::Scheduler,
};
pub use internal::virt::PageTableEntry;

/// Gets the page size for a given level.
pub fn get_page_size() -> usize {
    1 << get_page_bits()
}

/// Gets the highest possible shift for a canonical virtual address.
pub fn get_highest_bit_shift() -> usize {
    get_level_bits() * get_num_levels() + get_page_bits()
}

/// Gets the amount of bits in a page.
pub fn get_page_bits() -> usize {
    internal::virt::get_page_bits()
}

/// Gets the amount of bits contained in a single page level.
pub fn get_level_bits() -> usize {
    internal::virt::get_level_bits()
}

/// Gets the highest supported mappable page level.
/// This is different from [`get_num_levels`], because not all levels can be PTE leaves.
pub fn get_max_leaf_level() -> usize {
    internal::virt::get_max_leaf_level()
}

/// Gets the amount of page levels in a virtual address.
pub fn get_num_levels() -> usize {
    internal::virt::get_num_levels()
}

pub fn get_hhdm_base() -> VirtAddr {
    internal::virt::get_hhdm_base()
}

pub fn get_pfndb_base() -> VirtAddr {
    internal::virt::get_pfndb_base()
}

pub fn get_map_base() -> VirtAddr {
    internal::virt::get_map_base()
}

/// Sets a given page table as the active one on this CPU.
///
/// # Safety
///
/// The caller must make sure that all kernel pages are still mapped as they were before.
pub unsafe fn set_page_table(pt: &PageTable) {
    unsafe { internal::virt::set_page_table(pt) };
}

/// Invalidates a TLB entry cache.
pub fn flush_tlb(addr: VirtAddr) {
    internal::virt::flush_tlb(addr);
}

/// Returns true if the virtual address is a valid userspace address.
pub fn is_user_addr(addr: VirtAddr) -> bool {
    internal::virt::is_user_addr(addr)
}

pub unsafe fn set_user_access(state: bool) {
    unsafe { internal::virt::set_user_access(state) }
}

/// Performs a memcpy from user to kernel memory. Returns true if the access was successful.
/// # Safety
/// `dest` must be a valid kernel memory address.
#[must_use]
pub fn copy_from_user(dest: &mut [u8], src: VirtAddr) -> bool {
    if dest.len() == 0 {
        return true;
    }
    let task = Scheduler::get_current();
    let ptr = task.uar.as_ptr();
    unsafe {
        set_user_access(true);
        let result = internal::virt::copy_from_user(dest.as_mut_ptr(), src, dest.len(), ptr);
        set_user_access(false);
        result
    }
}

/// Performs a memcpy from kernel to user memory. Returns true if the access was successful.
/// # Safety
/// `src` must be a valid kernel memory address.
#[must_use]
pub fn copy_to_user(dest: VirtAddr, src: &[u8]) -> bool {
    if src.len() == 0 {
        return true;
    }
    let task = Scheduler::get_current();
    let ptr = task.uar.as_ptr();
    unsafe {
        set_user_access(true);
        let result = internal::virt::copy_to_user(dest, src.as_ptr(), src.len(), ptr);
        set_user_access(false);
        result
    }
}

/// Performs a strnlen on a C string in user memory. Returns the amount of bytes in this string.
#[must_use]
pub fn cstr_len_user(src: VirtAddr, max_len: usize) -> Option<usize> {
    if max_len == 0 {
        return Some(0);
    }
    let task = Scheduler::get_current();
    let ptr = task.uar.as_ptr();
    let mut count = 0;

    if unsafe {
        set_user_access(true);
        let result = internal::virt::cstr_len_user(src, max_len, &raw mut count, ptr);
        set_user_access(false);
        result
    } {
        Some(count)
    } else {
        None
    }
}

// # Note
// This module is only used to ensure the API is correctly implemented,
// since associated functions are more complicated. Not to be used directly.
#[doc(hidden)]
#[allow(unused)]
mod api {
    use super::PageTableEntry;
    use crate::memory::{PhysAddr, virt::PteFlags};

    /// Returns a PTE which represents an empty slot.
    const fn pte_empty() -> PageTableEntry {
        PageTableEntry::empty()
    }

    /// Returns a new PTE with a set address.
    const fn pte_new(address: PhysAddr, flags: PteFlags, level: usize) -> PageTableEntry {
        PageTableEntry::new(address, flags, level)
    }

    /// Returns the inner representation of the PTE.
    const fn pte_inner(pte: &PageTableEntry) -> usize {
        PageTableEntry::inner(pte)
    }

    /// Returns true if the PTE is present.
    fn pte_is_present(pte: &PageTableEntry) -> bool {
        pte.is_present()
    }

    /// Returns true if the PTE is a directory, aka not a leaf entry.
    fn pte_is_directory(pte: &PageTableEntry, level: usize) -> bool {
        pte.is_directory(level)
    }

    fn pte_is_dirty(pte: &PageTableEntry) -> bool {
        pte.is_dirty()
    }

    /// Returns the contained address pointed to by the PTE.
    fn pte_address(pte: &PageTableEntry) -> PhysAddr {
        pte.address()
    }
}
