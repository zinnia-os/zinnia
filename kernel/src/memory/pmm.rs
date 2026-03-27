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
use core::{
    hint::unlikely,
    ptr::{NonNull, null_mut, write_bytes},
    slice,
    sync::atomic::{AtomicPtr, Ordering},
};

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
    unsafe fn dealloc_bytes(addr: PhysAddr, bytes: usize) {
        let pages = divide_up(bytes, arch::virt::get_page_size());
        return unsafe { Self::dealloc(addr, pages) };
    }
}

// WARNING: Keep this structure as small as possible, every single physical page has one!
/// Metadata about a physical page.
#[derive(Debug)]
#[repr(C)]
pub struct Page {
    pub next: Option<NonNull<Page>>,
    pub count: usize,
}

unsafe impl Send for Page {}
unsafe impl Sync for Page {}

// If this assert fails, the PFNDB can't properly allocate data.
static_assert!(0x1000 % size_of::<Page>() == 0);

pub static PAGE_DB: SpinMutex<&'static mut [Page]> = SpinMutex::new(&mut []);
pub static PAGE_DB_START: AtomicPtr<()> = AtomicPtr::new(null_mut());

struct Pmm {
    head: Option<NonNull<Page>>,
}

unsafe impl Send for Pmm {}
unsafe impl Sync for Pmm {}

static PMM: SpinMutex<Pmm> = SpinMutex::new(Pmm { head: None });

pub struct KernelAlloc;
impl PageAllocator for KernelAlloc {
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
        let mut prev_it: Option<NonNull<Page>> = None;
        while let Some(mut x) = it {
            let page = unsafe { x.as_mut() };

            if page.get_address() + bytes >= limit {
                prev_it = it;
                it = page.next;
                continue;
            }

            if unlikely(page.count < pages) {
                prev_it = it;
                it = page.next;
                continue;
            }

            if unlikely(page.count == pages) {
                addr = Some(page.get_address());
                if let Some(mut prev) = prev_it {
                    let prev_page = unsafe { prev.as_mut() };
                    prev_page.next = page.next;
                } else {
                    head.head = page.next;
                }
                page.next = None;
                page.count = 0;
            } else {
                page.count -= pages;
                addr = Some(page.get_address() + page.count * arch::virt::get_page_size());
            }
            break;
        }

        // TODO: Merge adjacent regions if we didn't find anything.
        debug_assert!(addr.is_some());

        match addr {
            Some(x) => {
                if !flags.contains(AllocFlags::NoZero) {
                    unsafe { write_bytes(addr.unwrap().as_hhdm() as *mut u8, 0, bytes) };
                }
                Ok(x)
            }
            None => Err(AllocError),
        }
    }

    unsafe fn dealloc(addr: PhysAddr, pages: usize) {
        // If we have an empty allocation, there's nothing to free.
        if pages == 0 {
            return;
        }

        let mut head = PMM.lock();
        let mut page_db = PAGE_DB.lock();
        let page = page_db.get_mut(Page::idx_from_addr(addr)).unwrap();

        debug_assert!(page.count == 0);
        debug_assert!(page.next.is_none());

        page.count = pages;
        page.next = head.head;
        head.head = NonNull::new(page);
    }
}

impl Page {
    #[inline]
    pub fn idx_from_addr(address: PhysAddr) -> usize {
        address.0 / arch::virt::get_page_size()
    }

    /// Returns the page number of this page.
    fn get_pn(&self) -> usize {
        let page: *const Page = self;
        let page = page as usize;
        let db = PAGE_DB_START.load(Ordering::Relaxed) as usize;
        debug_assert!(page >= db, "{page:#018x} >= {db:#018x}");
        return (page - db) / size_of::<Page>();
    }

    #[inline]
    fn get_address(&self) -> PhysAddr {
        (self.get_pn() << arch::virt::get_page_bits()).into()
    }
}

/// Initializes the phyiscal memory manager.
pub fn init(memory_map: &[PhysMemory], pages: (*mut Page, usize)) {
    PAGE_DB_START.store(pages.0 as _, Ordering::Release);
    *PAGE_DB.lock() = unsafe { slice::from_raw_parts_mut(pages.0, pages.1) };

    let mut total_memory = 0;

    // Register free regions.
    for entry in memory_map.iter() {
        if entry.length < arch::virt::get_page_size() || entry.usage != PhysMemoryUsage::Usable {
            continue;
        }

        let mut pmm = PMM.lock();
        let mut page_db = PAGE_DB.lock();
        let page = page_db.get_mut(Page::idx_from_addr(entry.address)).unwrap();
        page.count = entry.length / arch::virt::get_page_size();
        page.next = pmm.head;
        pmm.head = NonNull::new(page);

        total_memory += entry.length;
    }

    log!("Total available memory: {} MiB", total_memory / 1024 / 1024);
}
