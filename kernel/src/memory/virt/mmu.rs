use crate::{
    arch::{self, virt::PageTableEntry},
    {
        memory::{
            PhysAddr, VirtAddr,
            pmm::{AllocFlags, KernelAlloc, PageAllocator},
            virt::{KERNEL_MMAP_BASE_ADDR, KERNEL_PAGE_TABLE, PageTableError, PteFlags, VmFlags},
        },
        util::{align_up, mutex::spin::SpinMutex},
    },
};
use alloc::{alloc::AllocError, slice};
use core::sync::atomic::Ordering;

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

        // Increase mapping base.
        // TODO: Use actual virtual address allocator.
        let virt = KERNEL_MMAP_BASE_ADDR.fetch_add(aligned_len, Ordering::SeqCst);

        // Map memory.
        self.map_range::<P>(VirtAddr(virt), phys, flags, aligned_len)
            .map_err(|_| AllocError)?;
        return Ok(virt as *mut u8);
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
        let mut current_head: *mut PageTableEntry = head.as_hhdm();
        let mut index = 0;
        let mut do_break = false;

        // Traverse the page table (from highest to lowest level).
        for level in (0..self.root_level).rev() {
            // Create a mask for the address part of the PTE, e.g. 0x1ff for 9 bits.
            let addr_bits = usize::MAX >> (usize::BITS as usize - arch::virt::get_level_bits());

            // Determine the shift for the appropriate level, e.g. x << (12 + (9 * level)).
            let addr_shift = arch::virt::get_page_bits() + (arch::virt::get_level_bits() * level);

            // Get the index for this level by masking the relevant address part.
            index = (virt.0 >> addr_shift) & addr_bits;

            // The last level is used to access the actual PTE, so break the loop then.
            if level == 0 || do_break {
                break;
            }

            unsafe {
                let pte = current_head.add(index);

                let mut pte_flags = PteFlags::Directory
                    | if self.is_user {
                        PteFlags::User
                    } else {
                        PteFlags::empty()
                    };

                if (*pte).is_present() {
                    // If this PTE is a large page, it already contains the final address. Don't continue.
                    if !(*pte).is_directory(level) {
                        pte_flags |= PteFlags::Large;
                        do_break = true;
                    } else {
                        // If the PTE is not large, go one level deeper.
                        current_head = (*pte).address().as_hhdm();
                    }
                    *pte = PageTableEntry::new((*pte).address(), pte_flags, level);
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

        Ok(unsafe { current_head.add(index) })
    }

    /// Establishes a new mapping in this page table.
    /// Fails if the mapping already exists. To overwrite a mapping, use [`Self::remap_single`] instead.
    pub fn map_single<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: VmFlags,
    ) -> Result<(), PageTableError> {
        let pte = self.get_pte::<P>(virt, true)?;

        unsafe {
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
        };

        return Ok(());
    }

    /// Changes the permissions on a mapping.
    pub fn remap_single<P: PageAllocator>(
        &self,
        virt: VirtAddr,
        flags: VmFlags,
    ) -> Result<(), PageTableError> {
        let pte = self.get_pte::<P>(virt, false)?;

        unsafe { *pte = PageTableEntry::new((*pte).address(), flags.as_pte(), 0) };
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
        let pte = self.get_pte::<P>(virt, false)?;
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
        self.get_pte::<KernelAlloc>(virt, false)
            .map(|x| {
                let pte = unsafe { x.read_volatile() };
                pte.is_present()
            })
            .unwrap_or(false)
    }
}
