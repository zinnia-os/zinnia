use crate::{
    arch,
    memory::{
        AllocFlags, KERNEL_PAGE_TABLE, KERNEL_VIRTUAL_ALLOCATOR, KernelAlloc, PageAllocator,
        PhysAddr, VirtAddr, VmCacheType, VmFlags, virt,
    },
    posix::errno::{EResult, Errno},
};
use alloc::vec::Vec;
use core::num::NonZero;

pub struct KernelStack {
    base: VirtAddr,
    top: VirtAddr,
}

impl KernelStack {
    /// Size of the kernel stack, including the guard page.
    const SIZE: NonZero<usize> = NonZero::new(32 * 1024).unwrap(); // 32 KiB

    pub fn new() -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        let virtual_address = KERNEL_VIRTUAL_ALLOCATOR.get().lock().allocate(Self::SIZE)?;
        let page_table = KERNEL_PAGE_TABLE.get();

        for i in (page_size..Self::SIZE.get()).step_by(page_size) {
            let physical_page = KernelAlloc::alloc(1, AllocFlags::empty())?;
            page_table
                .map_single::<KernelAlloc>(
                    virtual_address + i,
                    physical_page,
                    VmFlags::Read | VmFlags::Write,
                    VmCacheType::Normal,
                )
                .map_err(|_| Errno::ENOMEM)?;
        }

        Ok(Self {
            base: virtual_address,
            top: virtual_address + Self::SIZE.get(),
        })
    }

    pub fn top(&self) -> VirtAddr {
        self.top
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let page_size = arch::virt::get_page_size();
        let page_table = KERNEL_PAGE_TABLE.get();
        let mut pages = Vec::<PhysAddr>::new();

        for i in (page_size..Self::SIZE.get()).step_by(page_size) {
            let phys = page_table.get_mapping(self.base + i).unwrap().unwrap();
            page_table
                .unmap_single_no_shootdown::<KernelAlloc>(self.base + i)
                .unwrap();
            pages.push(phys);
        }

        virt::shootdown::submit_shootdown(page_table, self.base.value(), Self::SIZE.get());

        for phys in pages {
            unsafe { KernelAlloc::dealloc(phys, 1) };
        }

        KERNEL_VIRTUAL_ALLOCATOR
            .get()
            .lock()
            .release(self.base, Self::SIZE)
            .unwrap();
    }
}
