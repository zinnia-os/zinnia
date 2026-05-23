use super::PhysAddr;
use crate::{
    arch,
    {
        boot::{PhysMemory, PhysMemoryUsage},
        util::{divide_up, mutex::spin::SpinMutex},
    },
};
use alloc::alloc::AllocError;
use bitflags::bitflags;
use core::{hint::unlikely, panic::Location};

bitflags! {
    #[derive(Debug)]
    pub struct AllocFlags: usize {
        /// Only consider physical memory below 1MiB.
        const Kernel20 = 1 << 0;
        /// Only consider physical memory below 4GiB.
        const Kernel32 = 1 << 1;
        /// Do not initialize allocated memory to zero.
        const NoZero = 1 << 2;
    }
}

pub trait PageAllocator {
    /// Allocates `pages` amount of consecutive pages.
    fn alloc(pages: usize, flags: AllocFlags) -> Result<PhysAddr, AllocError>;

    /// Allocates enough consecutive pages to fit `bytes` amount of bytes.
    fn alloc_bytes(bytes: usize, flags: AllocFlags) -> Result<PhysAddr, AllocError> {
        let pages = divide_up(bytes, arch::virt::get_page_size());
        return Self::alloc(pages, flags);
    }

    /// Deallocates a region of `pages` amount of consecutive pages.
    /// # Safety
    /// Deallocating arbitrary physical addresses is inherently unsafe, since it can cause the kernel to corrupt.
    unsafe fn dealloc(addr: PhysAddr, pages: usize);

    /// Deallocates enough consecutive pages to fit `bytes` amount of bytes.
    /// # Safety
    /// To be completely safe, only use this function together with [`PageAllocator::alloc_bytes`].
    /// Safety notes from [`PageAllocator::dealloc`] also apply here.
    unsafe fn dealloc_bytes(addr: PhysAddr, bytes: usize);
}

// WARNING: Keep this structure as small as possible, every single physical page has one!
/// Metadata about a physical page.
#[derive(Debug)]
#[repr(C)]
pub struct Page {
    pub next: usize,
    pub count: usize,
}

// If this assert fails, the PFNDB can't properly allocate data.
static_assert!(0x1000 % size_of::<Page>() == 0);

const NO_PAGE: usize = usize::MAX;

struct Pmm {
    head: usize,
    pages: &'static mut [Page],
}

static PMM: SpinMutex<Pmm> = SpinMutex::new(Pmm {
    head: NO_PAGE,
    pages: &mut [],
});

pub struct KernelAlloc;
impl KernelAlloc {
    fn dealloc_inner(addr: PhysAddr, pages: usize) {
        // If we have an empty allocation, there's nothing to free.
        if pages == 0 {
            return;
        }

        let mut pmm = PMM.lock();
        let idx = Page::idx_from_addr(addr);
        let old_head = pmm.head;
        let page = pmm.pages.get_mut(idx).unwrap();

        debug_assert!(page.count == 0);
        debug_assert!(page.next == NO_PAGE);

        page.count = pages;
        page.next = old_head;
        pmm.head = idx;
    }
}

impl PageAllocator for KernelAlloc {
    #[track_caller]
    fn alloc(pages: usize, flags: AllocFlags) -> Result<PhysAddr, AllocError> {
        let mut head = PMM.lock();
        let bytes = pages * arch::virt::get_page_size();

        let limit = if flags.contains(AllocFlags::Kernel20) {
            PhysAddr(1 << 20)
        } else if flags.contains(AllocFlags::Kernel32) {
            PhysAddr(1 << 32)
        } else {
            PhysAddr(usize::MAX)
        };

        let mut addr = None;
        let mut it = head.head;
        let mut prev_it = None;
        while it != NO_PAGE {
            let idx = it;
            let page_addr = Page::addr_from_idx(idx);
            let page_count = head.pages[idx].count;
            let next = head.pages[idx].next;

            if page_addr + bytes >= limit {
                prev_it = Some(idx);
                it = next;
                continue;
            }

            if unlikely(page_count < pages) {
                prev_it = Some(idx);
                it = next;
                continue;
            }

            if unlikely(page_count == pages) {
                addr = Some(page_addr);
                if let Some(prev) = prev_it {
                    head.pages[prev].next = next;
                } else {
                    head.head = next;
                }
                head.pages[idx].next = NO_PAGE;
                head.pages[idx].count = 0;
            } else {
                head.pages[idx].count -= pages;
                addr = Some(page_addr + head.pages[idx].count * arch::virt::get_page_size());
            }
            break;
        }

        // TODO: Merge adjacent regions if we didn't find anything.
        debug_assert!(addr.is_some());

        match addr {
            Some(x) => {
                if !flags.contains(AllocFlags::NoZero) {
                    x.zero_hhdm(bytes);
                }
                Ok(x)
            }
            None => {
                error!("Unable to allocate for \"{}\"", Location::caller());
                Err(AllocError)
            }
        }
    }

    #[track_caller]
    unsafe fn dealloc(addr: PhysAddr, pages: usize) {
        Self::dealloc_inner(addr, pages);
    }

    #[track_caller]
    unsafe fn dealloc_bytes(addr: PhysAddr, bytes: usize) {
        let pages = divide_up(bytes, arch::virt::get_page_size());
        Self::dealloc_inner(addr, pages);
    }
}

impl Page {
    #[inline]
    pub fn idx_from_addr(address: PhysAddr) -> usize {
        address.0 / arch::virt::get_page_size()
    }

    #[inline]
    fn addr_from_idx(idx: usize) -> PhysAddr {
        (idx << arch::virt::get_page_bits()).into()
    }
}

/// Initializes the phyiscal memory manager.
pub fn init(memory_map: &[PhysMemory], pages: &'static mut [Page]) {
    let mut total_memory = 0;
    let mut pmm = PMM.lock();
    pmm.pages = pages;
    pmm.head = NO_PAGE;

    for page in pmm.pages.iter_mut() {
        page.next = NO_PAGE;
        page.count = 0;
    }

    // Register free regions.
    for entry in memory_map.iter() {
        if entry.length < arch::virt::get_page_size() || entry.usage != PhysMemoryUsage::Usable {
            continue;
        }

        let idx = Page::idx_from_addr(entry.address);
        let old_head = pmm.head;
        let page = pmm.pages.get_mut(idx).unwrap();
        page.count = entry.length / arch::virt::get_page_size();
        page.next = old_head;
        pmm.head = idx;

        total_memory += entry.length;
    }

    log!("Total available memory: {} MiB", total_memory / 1024 / 1024);
}
