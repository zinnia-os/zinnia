// Slab allocator

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
    hint::{likely, unlikely},
    mem::size_of,
    ptr::null_mut,
};

#[derive(Debug)]
struct Slab {
    /// Size of one entry.
    ent_size: usize,
    head: SpinMutex<VirtAddr>,
}

#[repr(transparent)]
struct SlabHeader {
    slab: *const Slab,
}

struct SlabInfo {
    /// Amount of pages connected to this slab.
    num_pages: usize,
    /// Size of this slab.
    size: usize,
}

impl Slab {
    /// Creates a new, uninitialized slab.
    const fn new(size: usize) -> Self {
        Self {
            ent_size: size,
            head: SpinMutex::new(VirtAddr::null()),
        }
    }

    /// Initializes a slab.
    fn init(&self) {
        unsafe {
            // Calculate the amount of bytes we need to skip in order to be able to store a reference to the slab.
            let offset = align_up(size_of::<SlabHeader>(), self.ent_size);
            // That also means we need to deduct that amount here.
            let available_size = arch::virt::get_page_size() - offset;

            // Allocate memory for this slab.
            let mem = KernelAlloc::alloc(1, AllocFlags::empty()).expect("Out of memory");
            let mut head = mem.as_hhdm::<*mut ()>();

            // Get a reference to the start of the buffer.
            let ptr = head as *mut SlabHeader;
            // In that first entry, record a pointer to the head.
            (*ptr).slab = &raw const *self;
            // Now save that start to the slab.
            head = (head).byte_add(offset);

            let arr = head;
            let max = available_size / self.ent_size - 1;
            let fact = self.ent_size / size_of::<*mut ()>();

            for i in 0..max {
                *arr.add(i * fact) = arr.add((i + 1) * fact) as *mut ();
            }
            *arr.add(max * fact) = null_mut();

            *self.head.lock() = head.into();
        }
    }

    fn alloc(&self) -> *mut u8 {
        // Initialize the slab if not done already.
        if likely(*self.head.lock() == VirtAddr::null()) {
            self.init();
        }

        let mut head = self.head.lock();
        let old_free = head.value() as *mut *mut ();

        unsafe {
            *head = (*old_free).into();
        }

        return old_free as *mut u8;
    }

    fn free(&self, addr: *mut u8) {
        if unlikely(addr.is_null()) {
            return;
        }

        let new_head = addr as *mut *mut ();
        let mut head = self.head.lock();

        unsafe {
            *new_head = head.value() as *mut ();
        }
        *head = new_head.into();
    }
}

#[inline]
fn find_size(size: usize) -> Option<&'static Slab> {
    ALLOCATOR.slabs.iter().find(|&slab| slab.ent_size >= size)
}

#[repr(C, align(4096))]
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
        let slab = find_size(layout.size());
        if let Some(s) = slab {
            // The allocation fits within our defined slabs.
            let result = s.alloc();
            debug_assert!((result as usize).is_multiple_of(layout.align()));
            return result;
        }

        // The allocation won't fit within our defined slabs.
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
                let header =
                    align_down(ptr as usize, arch::virt::get_page_size()) as *mut SlabHeader;
                (*(*header).slab).free(ptr);
            }
        }
    }
}
