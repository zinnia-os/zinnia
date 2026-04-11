// We don't want to use the bump allocator anywhere after initial setup.
mod bump;
pub mod cache;
pub mod iovec;
pub mod pmm;
pub mod slab;
pub mod user;
pub mod view;
pub mod virt;

pub use cache::*;
pub use iovec::*;
pub use pmm::*;
pub use slab::*;
pub use user::*;
pub use view::*;
pub use virt::*;

use super::util::once::Once;
use crate::{
    arch::{self, virt::get_page_size},
    boot::{BootInfo, PhysMemoryUsage},
    memory::virt::mmu::PageTable,
    util::{align_down, align_up},
};
use alloc::sync::Arc;
use bump::BumpAllocator;
use bytemuck::AnyBitPattern;
use core::{
    alloc::{GlobalAlloc, Layout},
    ops::{Add, Sub},
    ptr::{self, NonNull},
    sync::atomic::Ordering,
};

static HHDM_START: Once<VirtAddr> = Once::new();

/// Represents a physical address. It can't be directly read from or written to.
#[repr(transparent)]
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, AnyBitPattern)]
pub struct PhysAddr(usize);

impl PhysAddr {
    pub fn as_hhdm<T>(self) -> *mut T {
        VirtAddr(self.0 + HHDM_START.get().0).as_ptr()
    }
}

/// Represents a virtual address. It can't be directly read from or written to.
/// Note: Not the same as a pointer. A `VirtAddr` might point into another
/// process's memory that is not mapped in the kernel.
#[repr(transparent)]
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, AnyBitPattern)]
pub struct VirtAddr(usize);

impl VirtAddr {
    pub fn as_ptr<T>(self) -> *mut T {
        return ptr::with_exposed_provenance_mut(self.0);
    }

    /// Returns the physical address mapped in the kernel for this [`VirtAddr`].
    pub fn as_hhdm(self) -> Option<PhysAddr> {
        return self.0.checked_sub(HHDM_START.get().0).map(PhysAddr);
    }
}

macro_rules! addr_impl {
    ($ty:ty) => {
        impl $ty {
            #[inline]
            pub const fn null() -> Self {
                Self(0)
            }

            pub const fn new(value: usize) -> Self {
                Self(value)
            }

            #[inline]
            pub const fn value(&self) -> usize {
                self.0
            }

            #[inline]
            pub const fn is_null(&self) -> bool {
                self.0 == 0
            }
        }

        impl From<usize> for $ty {
            fn from(addr: usize) -> Self {
                Self(addr)
            }
        }

        #[cfg(target_pointer_width = "32")]
        impl From<u32> for $ty {
            fn from(addr: u32) -> Self {
                Self(addr as usize)
            }
        }

        #[cfg(target_pointer_width = "32")]
        impl Into<u32> for $ty {
            fn into(self) -> u32 {
                self.0 as u32
            }
        }

        #[cfg(target_pointer_width = "64")]
        impl From<u64> for $ty {
            fn from(addr: u64) -> Self {
                Self(addr as usize)
            }
        }

        #[cfg(target_pointer_width = "64")]
        impl Into<u64> for $ty {
            fn into(self) -> u64 {
                self.0 as u64
            }
        }

        impl<T> From<*const T> for $ty {
            fn from(ptr: *const T) -> Self {
                Self(ptr as usize)
            }
        }

        impl<T> From<*mut T> for $ty {
            fn from(ptr: *mut T) -> Self {
                Self(ptr as usize)
            }
        }

        impl<T> From<NonNull<T>> for $ty {
            fn from(ptr: NonNull<T>) -> Self {
                Self(ptr.as_ptr() as usize)
            }
        }

        impl Add for $ty {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

        impl Sub for $ty {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }

        impl Add<usize> for $ty {
            type Output = Self;

            fn add(self, rhs: usize) -> Self::Output {
                Self(self.0 + rhs)
            }
        }

        impl Sub<usize> for $ty {
            type Output = Self;

            fn sub(self, rhs: usize) -> Self::Output {
                Self(self.0 - rhs)
            }
        }

        impl core::fmt::Debug for $ty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_fmt(format_args!("{:#x}", self.0))
            }
        }
    };
}

addr_impl!(PhysAddr);
addr_impl!(VirtAddr);

/// Equivalent of C `malloc`.
#[unsafe(no_mangle)]
pub extern "C" fn malloc(size: usize) -> *mut core::ffi::c_void {
    let mem =
        unsafe { ALLOCATOR.alloc(Layout::from_size_align(size, align_of::<usize>()).unwrap()) };
    mem as *mut core::ffi::c_void
}

/// Equivalent of C `free`.
/// # Safety
/// The caller must make sure that `ptr` is an address handed out by [`malloc`].
/// The caller must also assert that the same allocation is never freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ptr: *mut core::ffi::c_void, size: usize) {
    unsafe {
        ALLOCATOR.dealloc(
            ptr as *mut u8,
            Layout::from_size_align(size, align_of::<usize>()).unwrap(),
        )
    };
}

/// Bootstraps the memory allocators and kernel virtual page table.
#[initgraph::task(
    name = "generic.memory",
    depends = [crate::arch::EARLY_INIT_STAGE],
)]
pub fn MEMORY_STAGE() {
    let info = BootInfo::get();

    let kernel_phys = info
        .kernel_phys
        .expect("Kernel physical address should have been set");

    let paging_level = info
        .paging_level
        .expect("Paging level should have been set");

    let highest_phys = info
        .highest_phys
        .expect("Highest physical address should have been set");

    let hhdm_address = info
        .hhdm_address
        .expect("HHDM address should have been set");

    unsafe { HHDM_START.init(hhdm_address) };

    let mut memory_map = info.memory_map.lock();

    // Print the memory map.
    memory_map.iter().for_each(|x| {
        log!(
            "{:#018x} - {:#018x} ({:?})",
            x.address.0,
            x.address.0 + x.length,
            x.usage
        )
    });

    // Find the largest region there is for the bump allocator.
    let bump_region = memory_map
        .iter()
        .filter(|x| x.usage == PhysMemoryUsage::Usable)
        .max_by(|x, y| x.length.cmp(&y.length))
        .unwrap();

    bump::BUMP_CURRENT.store(
        align_up(bump_region.address.value(), arch::virt::get_page_size()),
        Ordering::Relaxed,
    );
    bump::BUMP_LENGTH.store(
        align_down(bump_region.length, arch::virt::get_page_size()),
        Ordering::Relaxed,
    );

    // Now we can start making page allocations!
    // ------------------------------------

    // Remap the kernel in our own page table.
    log!("Using {}-level paging for page table", paging_level);
    let table = PageTable::new_kernel::<BumpAllocator>(paging_level, AllocFlags::empty());

    let text_start = VirtAddr(&raw const virt::LD_TEXT_START as usize);
    let text_end = VirtAddr(&raw const virt::LD_TEXT_END as usize);
    let rodata_start = VirtAddr(&raw const virt::LD_RODATA_START as usize);
    let rodata_end = VirtAddr(&raw const virt::LD_RODATA_END as usize);
    let data_start = VirtAddr(&raw const virt::LD_DATA_START as usize);
    let data_end = VirtAddr(&raw const virt::LD_DATA_END as usize);
    let kernel_start = VirtAddr(&raw const virt::LD_KERNEL_START as usize);

    log!("Kernel starts at: {:#018x}", kernel_phys.0);

    table
        .map_range::<BumpAllocator>(
            text_start,
            PhysAddr(text_start.0 - kernel_start.0 + kernel_phys.0),
            VmFlags::Read | VmFlags::Exec,
            text_end.0 - text_start.0,
        )
        .expect("Unable to map the text segment");
    log!("Mapped text segment at {:#018x}", text_start.0);

    table
        .map_range::<BumpAllocator>(
            rodata_start,
            PhysAddr(rodata_start.0 - kernel_start.0 + kernel_phys.0),
            VmFlags::Read,
            rodata_end.0 - rodata_start.0,
        )
        .expect("Unable to map the rodata segment");
    log!("Mapped rodata segment at {:#018x}", rodata_start.0);

    table
        .map_range::<BumpAllocator>(
            data_start,
            PhysAddr(data_start.0 - kernel_start.0 + kernel_phys.0),
            VmFlags::Read | VmFlags::Write,
            data_end.0 - data_start.0,
        )
        .expect("Unable to map the data segment");
    log!("Mapped data segment at {:#018x}", data_start.0);

    // Map physical memory.
    memory_map
        .iter()
        .filter(|x| x.usage == PhysMemoryUsage::Usable || x.usage == PhysMemoryUsage::Reclaimable)
        .for_each(|entry| {
            table
                .map_range::<BumpAllocator>(
                    arch::virt::get_hhdm_base() + entry.address.value(),
                    entry.address,
                    VmFlags::Read | VmFlags::Write,
                    entry.length,
                )
                .expect("Unable to map HHDM region");
        });

    log!(
        "Mapped HHDM segment at {:#018x}",
        arch::virt::get_hhdm_base().value()
    );

    // We record metadata for every single page of available memory in a large array.
    // This array is contiguous in virtual memory, but is sparsely populated.
    // Only those array entries which represent usable memory are mapped.

    // The offset where we start mapping the page array.
    log!("Highest physical address is {:#018x}", highest_phys.0);
    let page_base = arch::virt::get_pfndb_base();
    let page_length = highest_phys.0 / size_of::<Page>();
    let page_size = get_page_size();

    memory_map
        .iter()
        .filter(|x| x.length != 0)
        .for_each(|entry| {
            let length = align_up((entry.length / page_size) * size_of::<Page>(), page_size);
            let virt = align_down(
                (page_base + entry.address.0 / page_size * size_of::<Page>()).value(),
                page_size,
            );

            for page in (0..=length).step_by(page_size) {
                table
                    .map_single::<BumpAllocator>(
                        (virt + page).into(),
                        BumpAllocator::alloc(1, AllocFlags::empty()).unwrap(),
                        VmFlags::Read | VmFlags::Write,
                    )
                    .unwrap();
            }
        });

    log!(
        "Initalized page array region at {:#018x}",
        page_base.value()
    );

    // Activate the new page table.
    unsafe { table.set_active() };
    log!("Kernel map is now active");

    unsafe { HHDM_START.init(arch::virt::get_hhdm_base()) };

    // Finally, make sure to mark the allocated memory as used before the real allocator looks at the memory map.
    let allocated_bytes = align_up(
        bump::BUMP_CURRENT.load(Ordering::Relaxed) - bump_region.address.value(),
        get_page_size(),
    );
    log!("Bump-allocated bytes: {:#x}", allocated_bytes);
    {
        let bump_region = memory_map
            .iter_mut()
            .max_by(|x, y| x.length.cmp(&y.length))
            .unwrap();
        bump_region.address.0 += allocated_bytes;
        bump_region.length -= allocated_bytes;
    }

    // Now we're done using the bump allocator.
    // ----------------------------------------

    // Initialize the physical memory allocator.
    pmm::init(&memory_map, (page_base.as_ptr(), page_length));

    // Save the page table.
    unsafe { virt::KERNEL_PAGE_TABLE.init(Arc::new(table)) };

    // Set the MMAP base to right after the page table. Make sure this lands on a new PTE so we can map regular pages.
    // TODO: Use a virtual memory allocator instead.
    virt::KERNEL_MMAP_BASE_ADDR.store(arch::virt::get_map_base().value(), Ordering::Relaxed);
}
