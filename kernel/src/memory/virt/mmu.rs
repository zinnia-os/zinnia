use crate::{
    arch::{self, virt::PageTableEntry},
    {
        memory::{
            PhysAddr, VirtAddr,
            pmm::{AllocFlags, KernelAlloc, PageAllocator},
            virt::{
                KERNEL_PAGE_TABLE, KERNEL_VIRTUAL_ALLOCATOR, PageTableError, PteFlags, VmFlags,
            },
        },
        util::{align_up, mutex::spin::SpinMutex},
    },
};
use alloc::{alloc::AllocError, slice};
use core::num::NonZeroUsize;

/// Represents a virtual address space.
#[derive(Debug)]
pub struct PageTable {
    /// Physical address of the root directory.
    head: SpinMutex<PhysAddr>,
    /// The root page level.
    root_level: usize,
    /// `true`, if this is a user page table.
    is_user: bool,
}

impl PageTable {
    /// Creates a new page table for a user process.
    pub fn new_user<P: PageAllocator>(flags: AllocFlags) -> Self {
        // We need to have the higher half mapped in every user map for this to work.
        let user_l1 = P::alloc(1, flags).unwrap();
        unsafe {
            let user_l1_slice: &mut [u8] =
                slice::from_raw_parts_mut(user_l1.as_hhdm(), arch::virt::get_page_size());
            let kernel_l1_slice: &mut [u8] = slice::from_raw_parts_mut(
                KERNEL_PAGE_TABLE.get().head.lock().as_hhdm(),
                arch::virt::get_page_size(),
            );
            user_l1_slice.copy_from_slice(kernel_l1_slice);
        }
        Self {
            head: SpinMutex::new(user_l1),
            root_level: KERNEL_PAGE_TABLE.get().root_level,
            is_user: true,
        }
    }

    /// Creates a new page table for a kernel process.
    pub fn new_kernel<P: PageAllocator>(root_level: usize, flags: AllocFlags) -> Self {
        Self {
            head: SpinMutex::new(P::alloc(1, flags).unwrap()),
            root_level,
            is_user: false,
        }
    }

    pub fn get_kernel() -> &'static PageTable {
        KERNEL_PAGE_TABLE.get()
    }

    /// Maps physical memory to a free area in virtual address space.
    pub fn map_memory<P: PageAllocator>(
        &self,
        phys: PhysAddr,
        flags: VmFlags,
        length: usize,
    ) -> Result<*mut u8, AllocError> {
        let aligned_len = align_up(length, arch::virt::get_page_size());
        let len = NonZeroUsize::new(aligned_len).ok_or(AllocError)?;

        let virt = KERNEL_VIRTUAL_ALLOCATOR
            .get()
            .lock()
            .allocate(len)
            .map_err(|_| AllocError)?;

        if self.map_range::<P>(virt, phys, flags, aligned_len).is_err() {
            _ = KERNEL_VIRTUAL_ALLOCATOR.get().lock().release(virt, len);
            return Err(AllocError);
        }

        return Ok(virt.as_ptr());
    }

    /// Unmaps memory previously mapped by [`Self::map_memory`].
    pub fn unmap_memory<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        length: usize,
    ) -> Result<(), PageTableError> {
        let aligned_len = align_up(length, arch::virt::get_page_size());
        let Some(len) = NonZeroUsize::new(aligned_len) else {
            return Ok(());
        };

        self.unmap_range::<P>(virt, aligned_len)?;
        _ = KERNEL_VIRTUAL_ALLOCATOR.get().lock().release(virt, len);
        Ok(())
    }
}

impl PageTable {
    /// Returns the physical address of the top level.
    pub fn get_head_addr(&self) -> PhysAddr {
        unsafe { *self.head.raw_inner() }
    }

    pub const fn root_level(&self) -> usize {
        self.root_level
    }

    /// Sets this page table as the active one.
    ///
    /// # Safety
    ///
    /// All parts of the kernel must still be mapped for this call to be safe.
    pub unsafe fn set_active(&self) {
        unsafe {
            arch::virt::set_page_table(self);
        }
    }

    /// Gets the page table entry pointed to by `virt`.
    /// Allocates new levels if necessary and requested.
    pub fn get_pte<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        allocate: bool,
    ) -> Result<*mut PageTableEntry, PageTableError> {
        let head = self.head.lock();
        self.get_pte_locked::<P>(*head, virt, allocate)
    }

    fn get_pte_locked<P: PageAllocator>(
        &self,
        head: PhysAddr,
        virt: VirtAddr,
        allocate: bool,
    ) -> Result<*mut PageTableEntry, PageTableError> {
        let mut current_head: *mut PageTableEntry = head.as_hhdm();

        // Traverse the page table (from highest to lowest level).
        for level in (0..self.root_level).rev() {
            // Create a mask for the address part of the PTE, e.g. 0x1ff for 9 bits.
            let addr_bits = usize::MAX >> (usize::BITS as usize - arch::virt::get_level_bits());

            // Determine the shift for the appropriate level, e.g. x << (12 + (9 * level)).
            let addr_shift = arch::virt::get_page_bits() + (arch::virt::get_level_bits() * level);

            // Get the index for this level by masking the relevant address part.
            let index = (virt.0 >> addr_shift) & addr_bits;
            let pte = unsafe { current_head.add(index) };

            // The last level is used to access the actual PTE, so break the loop then.
            if level == 0 {
                return Ok(pte);
            }

            unsafe {
                let pte_flags = PteFlags::Directory
                    | if self.is_user {
                        PteFlags::User
                    } else {
                        PteFlags::empty()
                    };

                let entry = pte.read_volatile();
                if entry.is_present() {
                    // If this PTE is a large page, it already contains the final address. Don't continue.
                    if !entry.is_directory(level) {
                        return Ok(pte);
                    }

                    // If the PTE is not large, go one level deeper.
                    current_head = entry.address().as_hhdm();
                } else {
                    // PTE isn't present, but we have to allocate a new level now.
                    if !allocate {
                        return Err(PageTableError::NeedAllocation);
                    }

                    // Allocate a new level.
                    let next_head = P::alloc(1, AllocFlags::empty())
                        .map_err(|_| PageTableError::OutOfMemory)?
                        .as_hhdm();

                    // ptr::byte_sub() doesn't allow taking higher half addresses because it doesn't fit in an isize.
                    *pte = PageTableEntry::new(
                        VirtAddr::from(next_head)
                            .as_hhdm()
                            .ok_or(PageTableError::PageTableEntryMissing)?,
                        pte_flags,
                        level,
                    );
                    current_head = next_head;
                }
            }
        }

        unreachable!()
    }

    fn with_pte<P: PageAllocator, T>(
        &self,
        virt: VirtAddr,
        allocate: bool,
        f: impl FnOnce(&mut PageTableEntry) -> T,
    ) -> Result<T, PageTableError> {
        let head = self.head.lock();
        let pte = self.get_pte_locked::<P>(*head, virt, allocate)?;
        Ok(f(unsafe { &mut *pte }))
    }

    /// Establishes a new mapping in this page table.
    /// Fails if the mapping already exists. To overwrite a mapping, use [`Self::remap_single`] instead.
    pub fn map_single<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: VmFlags,
    ) -> Result<(), PageTableError> {
        self.with_pte::<P, _>(virt, true, |pte| {
            *pte = PageTableEntry::new(
                phys,
                flags.as_pte()
                    | if self.is_user {
                        PteFlags::User
                    } else {
                        PteFlags::empty()
                    },
                0,
            )
        })?;

        return Ok(());
    }

    /// Changes the permissions on a mapping.
    pub fn remap_single<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        flags: VmFlags,
    ) -> Result<(), PageTableError> {
        self.with_pte::<P, _>(virt, false, |pte| {
            *pte = PageTableEntry::new(
                pte.address(),
                flags.as_pte()
                    | if self.is_user {
                        PteFlags::User
                    } else {
                        PteFlags::empty()
                    },
                0,
            )
        })?;
        crate::arch::virt::flush_tlb(virt);

        return Ok(());
    }

    /// Maps a range of consecutive memory in this page table.
    pub fn map_range<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: VmFlags,
        length: usize,
    ) -> Result<(), PageTableError> {
        // TODO: Do transactional mapping.
        let length = align_up(length, arch::virt::get_page_size());
        let step = arch::virt::get_page_size();

        for offset in (0..length).step_by(step) {
            self.map_single::<P>(VirtAddr(virt.0 + offset), PhysAddr(phys.0 + offset), flags)?;
        }
        return Ok(());
    }

    /// Changes the permissions on a mapping of consecutive memory.
    pub fn remap_range<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        flags: VmFlags,
        length: usize,
    ) -> Result<(), PageTableError> {
        // TODO: Do transactional mapping.
        let length = align_up(length, arch::virt::get_page_size());
        let step = arch::virt::get_page_size();

        for offset in (0..length).step_by(step) {
            self.remap_single::<P>(VirtAddr(virt.0 + offset), flags)?;
        }
        return Ok(());
    }

    /// Un-maps a page from this page table.
    pub fn unmap_single<P: PageAllocator>(&self, virt: VirtAddr) -> Result<(), PageTableError> {
        let head = self.head.lock();
        let mut current_head: *mut PageTableEntry = head.as_hhdm();

        let mut pte = current_head;
        for level in (0..self.root_level).rev() {
            let addr_bits = usize::MAX >> (usize::BITS as usize - arch::virt::get_level_bits());
            let addr_shift = arch::virt::get_page_bits() + (arch::virt::get_level_bits() * level);
            let index = (virt.0 >> addr_shift) & addr_bits;

            pte = unsafe { current_head.add(index) };
            if level == 0 {
                break;
            }

            let entry = unsafe { pte.read_volatile() };
            if !entry.is_present() {
                return Err(PageTableError::NeedAllocation);
            }

            if !entry.is_directory(level) {
                break;
            }

            current_head = entry.address().as_hhdm();
        }

        unsafe {
            pte.write_volatile(PageTableEntry::empty());
        };
        crate::arch::virt::flush_tlb(virt);

        Ok(())
    }

    /// Un-maps a range from this page table.
    pub fn unmap_range<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        length: usize,
    ) -> Result<(), PageTableError> {
        // TODO: Do transactional mapping.
        let length = align_up(length, arch::virt::get_page_size());
        let step = arch::virt::get_page_size();
        for offset in (0..length).step_by(step) {
            self.unmap_single::<P>(VirtAddr(virt.0 + offset))?;
        }
        return Ok(());
    }

    /// Checks if the address (may be unaligned) is mapped in this page table.
    pub fn is_mapped(&self, virt: VirtAddr) -> bool {
        self.with_pte::<KernelAlloc, _>(virt, false, |pte| pte.is_present())
            .unwrap_or(false)
    }

    pub fn get_mapping(&self, virt: VirtAddr) -> Result<Option<PhysAddr>, PageTableError> {
        self.with_pte::<KernelAlloc, _>(virt, false, |pte| pte.is_present().then(|| pte.address()))
    }

    pub fn take_dirty(&self, virt: VirtAddr) -> bool {
        self.with_pte::<KernelAlloc, _>(virt, false, |pte| {
            if !pte.is_present() || !pte.is_dirty() {
                return false;
            }

            pte.clear_dirty();
            crate::arch::virt::flush_tlb(virt);
            true
        })
        .unwrap_or(false)
    }
}

impl PageTable {
    fn free_subtree(table: PhysAddr, level: usize) {
        if level == 0 {
            return;
        }

        let entries = 1usize << arch::virt::get_level_bits();
        let table_slice: &[PageTableEntry] =
            unsafe { slice::from_raw_parts(table.as_hhdm(), entries) };

        for pte in table_slice {
            if !pte.is_present() || !pte.is_directory(level) {
                continue;
            }

            let child = pte.address();
            Self::free_subtree(child, level - 1);
            unsafe { KernelAlloc::dealloc(child, 1) };
        }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        if !self.is_user {
            return;
        }

        let head = self.get_head_addr();
        let entries = 1usize << arch::virt::get_level_bits();
        let root_level = self.root_level.saturating_sub(1);
        let root_slice: &[PageTableEntry] =
            unsafe { slice::from_raw_parts(head.as_hhdm(), entries) };

        for pte in &root_slice[..entries / 2] {
            if !pte.is_present() || !pte.is_directory(root_level) {
                continue;
            }

            let child = pte.address();
            Self::free_subtree(child, root_level.saturating_sub(1));
            unsafe { KernelAlloc::dealloc(child, 1) };
        }

        unsafe { KernelAlloc::dealloc(head, 1) };
    }
}
