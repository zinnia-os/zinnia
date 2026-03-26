use crate::memory::{
    PhysAddr, UserAccessRegion, VirtAddr,
    virt::{PteFlags, mmu::PageTable},
};
use bitflags::bitflags;
use core::arch::asm;

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry {
    inner: u64,
}

impl core::fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let flags = PageFlags::from_bits_truncate(self.inner);
        f.debug_struct("PageTableEntry")
            .field("address", &self.address())
            .field("flags", &flags)
            .finish()
    }
}

/// Masks only the address bits of a PTE.
const PPN_MASK: u64 = ((1 << 44) - 1) << 10;

bitflags! {
    #[repr(transparent)]
    #[derive(Debug)]
    struct PageFlags: u64 {
        const Valid = 1 << 0;
        const Read = 1 << 1;
        const Write = 1 << 2;
        const Execute = 1 << 3;
        const UserMode = 1 << 4;
        const Global = 1 << 5;
        const Access = 1 << 6;
        const Dirty = 1 << 7;
    }
}

impl PageTableEntry {
    pub const fn empty() -> Self {
        return Self { inner: 0 };
    }

    pub const fn new(address: PhysAddr, flags: PteFlags, _level: usize) -> Self {
        let mut result = ((address.value() as u64 >> 2) & PPN_MASK) | PageFlags::Valid.bits();

        if flags.contains(PteFlags::User) {
            result |= PageFlags::UserMode.bits();
        }

        if flags.contains(PteFlags::Directory) {
            result |= 0;
        } else {
            result |= PageFlags::Read.bits();

            if flags.contains(PteFlags::Write) {
                result |= PageFlags::Write.bits();
            }

            if flags.contains(PteFlags::Exec) {
                result |= PageFlags::Execute.bits();
            }
        }

        Self { inner: result }
    }

    pub const fn inner(&self) -> usize {
        self.inner as usize
    }

    pub fn is_present(&self) -> bool {
        PageFlags::from_bits_truncate(self.inner).contains(PageFlags::Valid)
    }

    pub fn is_directory(&self, level: usize) -> bool {
        level > 0
            && !PageFlags::from_bits_truncate(self.inner)
                .intersects(PageFlags::Read | PageFlags::Write | PageFlags::Execute)
    }

    pub fn is_dirty(&self) -> bool {
        PageFlags::from_bits_truncate(self.inner).contains(PageFlags::Dirty)
    }

    pub fn address(&self) -> PhysAddr {
        ((self.inner & PPN_MASK) << 2).into()
    }
}

pub(in crate::arch) fn flush_tlb(addr: VirtAddr) {
    unsafe {
        asm!("sfence.vma {addr}, zero", addr = in(reg) addr.value());
    }
}

pub(in crate::arch) unsafe fn set_page_table(pt: &PageTable) {
    let satp = (pt.get_head_addr().value() >> 12)
        | (match pt.root_level() {
            3 => 8,
            4 => 9,
            5 => 10,
            _ => panic!("Root level out of range"),
        } << 60);
    unsafe {
        asm!("csrw satp, {satp}", satp = in(reg) satp);
    }
}

pub(in crate::arch) const fn get_page_bits() -> usize {
    12
}

pub(in crate::arch) const fn get_max_leaf_level() -> usize {
    3
}

pub(in crate::arch) const fn get_level_bits() -> usize {
    9
}

pub(in crate::arch) const fn get_num_levels() -> usize {
    3
}

pub(in crate::arch) fn get_hhdm_base() -> VirtAddr {
    VirtAddr::new(0xFFFF_8000_0000_0000)
}

pub(in crate::arch) fn get_pfndb_base() -> VirtAddr {
    VirtAddr::new(0xFFFF_A000_0000_0000)
}

pub(in crate::arch) fn get_map_base() -> VirtAddr {
    VirtAddr::new(0xFFFF_C000_0000_0000)
}

pub(in crate::arch) fn is_user_addr(addr: VirtAddr) -> bool {
    addr.value() < 0x0000_8000_0000_0000
}

pub(in crate::arch) unsafe extern "C" fn copy_from_user(
    dest: *mut u8,
    src: VirtAddr,
    len: usize,
    context: *mut *mut UserAccessRegion,
) -> bool {
    let _ = (dest, src, len, context);
    todo!()
}

pub(in crate::arch) unsafe extern "C" fn copy_to_user(
    dest: VirtAddr,
    src: *const u8,
    len: usize,
    context: *mut *mut UserAccessRegion,
) -> bool {
    let _ = (dest, src, len, context);
    todo!()
}

pub(in crate::arch) unsafe extern "C" fn cstr_len_user(
    src: VirtAddr,
    max_len: usize,
    count: *mut usize,
    context: *mut *mut UserAccessRegion,
) -> bool {
    let _ = (src, max_len, count, context);
    todo!()
}

pub(in crate::arch) unsafe fn set_user_access(state: bool) {
    let _ = state;
    todo!()
}
