use crate::{
    arch,
    memory::{
        AllocFlags, KERNEL_PAGE_TABLE, KERNEL_VIRTUAL_ALLOCATOR, KernelAlloc, PageAllocator,
        PhysAddr, VirtAddr, VmCacheType, VmFlags, virt,
    },
    posix::errno::{EResult, Errno},
};
use core::num::NonZero;

pub struct KernelStack {
    base: VirtAddr,
    top: VirtAddr,
}

impl KernelStack {
    const STACK_SIZE: usize = 0x7000;
    const PHYS_LIST_END: PhysAddr = PhysAddr::new(usize::MAX);

    pub fn new() -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        let guarded_size = Self::guarded_size();
        let virtual_address = KERNEL_VIRTUAL_ALLOCATOR
            .get()
            .lock()
            .allocate(guarded_size)?;
        let page_table = KERNEL_PAGE_TABLE.get();

        for i in (0..Self::STACK_SIZE).step_by(page_size) {
            let physical_page = KernelAlloc::alloc(1, AllocFlags::empty())?;
            page_table
                .map_single::<KernelAlloc>(
                    virtual_address + guarded_size.get() - Self::STACK_SIZE + i,
                    physical_page,
                    VmFlags::Read | VmFlags::Write,
                    VmCacheType::Normal,
                )
                .map_err(|_| Errno::ENOMEM)?;
        }

        Ok(Self {
            base: virtual_address,
            top: virtual_address + guarded_size.get(),
        })
    }

    pub fn top(&self) -> VirtAddr {
        self.top
    }

    fn guarded_size() -> NonZero<usize> {
        NonZero::new(Self::STACK_SIZE + arch::virt::get_page_size()).unwrap()
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let page_size = arch::virt::get_page_size();
        let page_table = KERNEL_PAGE_TABLE.get();
        let guarded_size = Self::guarded_size();

        let mut physical_stack = Self::PHYS_LIST_END;
        for i in (0..Self::STACK_SIZE).step_by(page_size) {
            let addr = self.base + guarded_size.get() - Self::STACK_SIZE + i;
            let phys = page_table.get_mapping(addr).unwrap().unwrap();
            page_table
                .unmap_single_no_shootdown::<KernelAlloc>(addr)
                .unwrap();
            unsafe { phys.as_hhdm::<PhysAddr>().write(physical_stack) };
            physical_stack = phys;
        }

        // Shoot down before freeing so no page is reused under a stale mapping.
        virt::shootdown::submit_shootdown(page_table, self.base.value(), guarded_size.get());
        while physical_stack != Self::PHYS_LIST_END {
            let next = unsafe { physical_stack.as_hhdm::<PhysAddr>().read() };
            unsafe { KernelAlloc::dealloc(physical_stack, 1) };
            physical_stack = next;
        }

        KERNEL_VIRTUAL_ALLOCATOR
            .get()
            .lock()
            .release(self.base, guarded_size)
            .unwrap();
    }
}
