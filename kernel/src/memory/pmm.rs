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

impl Pmm {
    fn free_region(&mut self, idx: usize, pages: usize) {
        debug_assert!(pages > 0);
        debug_assert!(self.pages[idx].count == 0);

        let mut prev: Option<usize> = None;
        let mut cur = self.head;
        while cur != NO_PAGE && cur < idx {
            prev = Some(cur);
            cur = self.pages[cur].next;
        }
        let next = if cur == NO_PAGE { None } else { Some(cur) };
        debug_assert!(next != Some(idx));

        // The previous region ends exactly where this one starts.
        if let Some(p) = prev
            && p + self.pages[p].count == idx
        {
            self.pages[p].count += pages;
            self.pages[idx].next = NO_PAGE;

            // Having grown `p`, it may now also be adjacent to `next`.
            if let Some(n) = next
                && p + self.pages[p].count == n
            {
                self.pages[p].count += self.pages[n].count;
                self.pages[p].next = self.pages[n].next;
                self.pages[n].count = 0;
                self.pages[n].next = NO_PAGE;
            }
            return;
        }

        if let Some(n) = next
            && idx + pages == n
        {
            let n_count = self.pages[n].count;
            let n_next = self.pages[n].next;
            self.pages[idx].count = pages + n_count;
            self.pages[idx].next = n_next;
            self.pages[n].count = 0;
            self.pages[n].next = NO_PAGE;
            match prev {
                Some(p) => self.pages[p].next = idx,
                None => self.head = idx,
            }
            return;
        }

        self.pages[idx].count = pages;
        self.pages[idx].next = next.unwrap_or(NO_PAGE);
        match prev {
            Some(p) => self.pages[p].next = idx,
            None => self.head = idx,
        }
    }
}

pub struct KernelAlloc;
impl KernelAlloc {
    fn dealloc_inner(addr: PhysAddr, pages: usize) {
        // If we have an empty allocation, there's nothing to free.
        if pages == 0 {
            return;
        }

        let mut pmm = PMM.lock();
        let idx = Page::idx_from_addr(addr);

        debug_assert!(pmm.pages[idx].count == 0);
        debug_assert!(pmm.pages[idx].next == NO_PAGE);

        pmm.free_region(idx, pages);
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
        let count = entry.length / arch::virt::get_page_size();
        pmm.free_region(idx, count);

        total_memory += entry.length;
    }

    log!("Total available memory: {} MiB", total_memory / 1024 / 1024);
}
