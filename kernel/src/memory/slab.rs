use super::{
    VirtAddr,
    pmm::{AllocFlags, KernelAlloc, PageAllocator},
};
use crate::{
    arch,
    util::{align_down, align_up, divide_up, mutex::spin::SpinMutex},
};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
    hint::unlikely,
    mem::size_of,
    ptr::null_mut,
};
use intrusive_collections::{LinkedList, LinkedListLink, UnsafeRef, intrusive_adapter};

#[repr(transparent)]
struct FreeSlot {
    next: *mut FreeSlot,
}

struct SlabHeader {
    link: LinkedListLink,
    slab: *const Slab,
    free: Cell<*mut FreeSlot>,
    used: Cell<usize>,
}

unsafe impl Send for SlabHeader {}
unsafe impl Sync for SlabHeader {}

intrusive_adapter!(SlabPageAdapter = UnsafeRef<SlabHeader>: SlabHeader { link => LinkedListLink });

struct SlabInfo {
    /// Amount of pages connected to this slab.
    num_pages: usize,
    /// Size of this slab.
    size: usize,
}

struct Slab {
    /// Size of one entry.
    ent_size: usize,
    /// Pages with at least one free slot.
    partial: SpinMutex<LinkedList<SlabPageAdapter>>,
}

impl Slab {
    /// Creates a new, uninitialized slab.
    const fn new(size: usize) -> Self {
        Self {
            ent_size: size,
            partial: SpinMutex::new(LinkedList::new(SlabPageAdapter::NEW)),
        }
    }

    /// Byte offset of the first slot in a page of this slab.
    #[inline]
    fn slot_offset(&self) -> usize {
        align_up(size_of::<SlabHeader>(), self.ent_size)
    }

    /// Allocates a fresh, unlinked page and chains its slots into the free list. Returns null if the PMM is exhausted.
    fn refill(&self) -> *mut SlabHeader {
        let Ok(mem) = KernelAlloc::alloc(1, AllocFlags::empty()) else {
            return null_mut();
        };

        let page = mem.as_hhdm::<SlabHeader>();
        let offset = self.slot_offset();
        let slots = (arch::virt::get_page_size() - offset) / self.ent_size;
        debug_assert!(slots > 0);

        unsafe {
            let first = (page as *mut u8).byte_add(offset) as *mut FreeSlot;
            for i in 0..slots - 1 {
                (*first.byte_add(i * self.ent_size)).next = first.byte_add((i + 1) * self.ent_size);
            }
            (*first.byte_add((slots - 1) * self.ent_size)).next = null_mut();

            page.write(SlabHeader {
                link: LinkedListLink::new(),
                slab: &raw const *self,
                free: Cell::new(first),
                used: Cell::new(0),
            });
        }

        page
    }

    fn alloc(&self) -> *mut u8 {
        let mut list = self.partial.lock();

        if unlikely(list.is_empty()) {
            let page = self.refill();
            if unlikely(page.is_null()) {
                return null_mut();
            }
            list.push_front(unsafe { UnsafeRef::from_raw(page) });
        }

        let mut cursor = list.front_mut();
        let page = cursor.get().unwrap();

        let slot = page.free.get();
        debug_assert!(!slot.is_null());
        page.free.set(unsafe { (*slot).next });
        page.used.set(page.used.get() + 1);

        // A page with no free slots left leaves the partial list.
        if page.free.get().is_null() {
            cursor.remove();
        }

        slot as *mut u8
    }

    /// # Safety
    /// `page` must be the header of the page containing `addr`.
    unsafe fn free(&self, page: *mut SlabHeader, addr: *mut u8) {
        if unlikely(addr.is_null()) {
            return;
        }

        let slot = addr as *mut FreeSlot;
        let mut list = self.partial.lock();

        let (was_linked, now_empty) = {
            let page = unsafe { &*page };
            let was_linked = page.link.is_linked();
            unsafe { (*slot).next = page.free.get() };
            page.free.set(slot);
            let used = page.used.get() - 1;
            page.used.set(used);
            (was_linked, used == 0)
        };

        if now_empty {
            // Drop it from the list and hand the page back to the PMM.
            if was_linked {
                unsafe { list.cursor_mut_from_ptr(page).remove() };
            }
            unsafe { KernelAlloc::dealloc(VirtAddr::from(page).as_hhdm().unwrap(), 1) };
        } else if !was_linked {
            list.push_front(unsafe { UnsafeRef::from_raw(page) });
        }
    }
}

#[inline]
fn find_size(size: usize) -> Option<&'static Slab> {
    ALLOCATOR.slabs.iter().find(|&slab| slab.ent_size >= size)
}

pub struct SlabAllocator {
    slabs: [Slab; 8],
}

// Register the slab allocator as the global allocator.
#[global_allocator]
pub static ALLOCATOR: SlabAllocator = SlabAllocator {
    slabs: [
        Slab::new(16),
        Slab::new(32),
        Slab::new(64),
        Slab::new(128),
        Slab::new(256),
        Slab::new(512),
        Slab::new(1024),
        Slab::new(2048),
    ],
};

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // If there's nothing to allocate, don't.
        if unlikely(layout.size() == 0) {
            return null_mut();
        }

        // Find a suitable slab.
        let effective = layout.size().max(layout.align());
        let slab = find_size(effective);
        if let Some(s) = slab {
            // The allocation fits within our defined slabs.
            let result = s.alloc();
            debug_assert!((result as usize).is_multiple_of(layout.align()));
            return result;
        }

        // The allocation won't fit within our defined slabs.
        debug_assert!(layout.align() <= arch::virt::get_page_size());
        // Get how many pages have to be allocated in order to fit `size`.
        let num_pages = divide_up(layout.size(), arch::virt::get_page_size());

        // Allocate the pages plus an additional page for metadata.
        match KernelAlloc::alloc(num_pages + 1, AllocFlags::empty()) {
            Ok(mem) => unsafe {
                // Convert the physical address to a pointer.
                let ret: *mut u8 = mem.as_hhdm();

                // Write metadata into the first page.
                let info = ret as *mut SlabInfo;
                (*info).num_pages = num_pages;
                (*info).size = layout.size();

                // Skip the metadata and return the next one.
                return ret.byte_add(arch::virt::get_page_size());
            },
            Err(_) => return null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        unsafe {
            if ptr as usize == align_down(ptr as usize, arch::virt::get_page_size()) {
                let info = ptr.sub(arch::virt::get_page_size()) as *mut SlabInfo;
                KernelAlloc::dealloc(
                    (VirtAddr::from(info)).as_hhdm().unwrap(),
                    (*info).num_pages + 1,
                );
            } else {
                let page = align_down(ptr as usize, arch::virt::get_page_size()) as *mut SlabHeader;
                (*(*page).slab).free(page, ptr);
            }
        }
    }
}
