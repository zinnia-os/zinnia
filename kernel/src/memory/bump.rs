//! Early memory setup and allocator.

use super::{
    PhysAddr,
    pmm::{AllocFlags, PageAllocator},
};
use crate::arch::{self};
use alloc::alloc::AllocError;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Points to the current address for the next allocation.
pub(crate) static BUMP_CURRENT: AtomicUsize = AtomicUsize::new(0);

/// The amount of memory left to be handed out by the allocator.
pub(crate) static BUMP_LENGTH: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct BumpAllocator;
impl PageAllocator for BumpAllocator {
    fn alloc(pages: usize, flags: AllocFlags) -> Result<PhysAddr, AllocError> {
        let bytes = pages * arch::virt::get_page_size();
        let old = BUMP_LENGTH.load(Ordering::Relaxed);

        if let Some(new) = old.checked_sub(bytes) {
            BUMP_LENGTH.store(new, Ordering::Relaxed);
            let mem = PhysAddr(BUMP_CURRENT.fetch_add(bytes, Ordering::Relaxed));

            if !flags.contains(AllocFlags::NoZero) {
                unsafe { (mem.as_hhdm() as *mut u8).write_bytes(0, bytes) };
            }
            return Ok(mem);
        }

        return Err(AllocError);
    }

    unsafe fn dealloc(_addr: PhysAddr, _pages: usize) {
        unimplemented!(
            "The bump allocator is not supposed to free anything. Remove this .dealloc()"
        )
    }
}
