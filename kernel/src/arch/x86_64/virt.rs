use crate::{
    arch::x86_64::ARCH_DATA,
    memory::{
        PhysAddr, UserAccessRegion, VirtAddr,
        virt::{PteFlags, mmu::PageTable},
    },
};
use bitflags::bitflags;
use core::arch::{asm, naked_asm};

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
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

bitflags! {
    #[repr(transparent)]
    #[derive(Debug)]
    struct PageFlags: u64 {
        const None = 0;
        const Present = 1 << 0;
        const ReadWrite = 1 << 1;
        const UserMode = 1 << 2;
        const WriteThrough = 1 << 3;
        const CacheDisable = 1 << 4;
        const Accessed = 1 << 5;
        const Dirty = 1 << 6;
        const Size = 1 << 7;
        const Global = 1 << 8;
        const Available = 1 << 9;
        const AttributeTable = 1 << 10;
        const ExecuteDisable = 1 << 63;
    }
}

impl PageTableEntry {
    pub const fn empty() -> Self {
        return Self { inner: 0 };
    }

    pub const fn new(address: PhysAddr, flags: PteFlags, _level: usize) -> Self {
        let mut result = (address.value() as u64 & ADDR_MASK) | PageFlags::Present.bits();

        if flags.contains(PteFlags::User) {
            result |= PageFlags::UserMode.bits();
        }

        if flags.contains(PteFlags::Directory) {
            result |= PageFlags::ReadWrite.bits();
        } else {
            if flags.contains(PteFlags::Write) {
                result |= PageFlags::ReadWrite.bits();
            }

            if !flags.contains(PteFlags::Exec) {
                result |= PageFlags::ExecuteDisable.bits();
            }

            if flags.contains(PteFlags::Large) {
                result |= PageFlags::Size.bits();
            }
        }

        Self { inner: result }
    }

    pub const fn inner(&self) -> usize {
        self.inner as usize
    }

    pub fn is_present(&self) -> bool {
        PageFlags::from_bits_retain(self.inner).contains(PageFlags::Present)
    }

    pub fn is_directory(&self, level: usize) -> bool {
        level > 0 && !PageFlags::from_bits_retain(self.inner).contains(PageFlags::Size)
    }

    pub fn is_dirty(&self) -> bool {
        PageFlags::from_bits_retain(self.inner).contains(PageFlags::Dirty)
    }

    pub fn address(&self) -> PhysAddr {
        (self.inner & ADDR_MASK).into()
    }
}

pub(in crate::arch) fn flush_tlb(addr: VirtAddr) {
    unsafe {
        asm!("invlpg [{addr}]", addr = in(reg) addr.value());
    }
}

pub(in crate::arch) unsafe fn set_page_table(pt: &PageTable) {
    unsafe {
        asm!("mov cr3, {addr}", addr = in(reg) pt.get_head_addr().value());
    }
}

pub(in crate::arch) const fn get_page_bits() -> usize {
    12
}

pub(in crate::arch) const fn get_max_leaf_level() -> usize {
    2
}

pub(in crate::arch) const fn get_level_bits() -> usize {
    9
}

pub(in crate::arch) const fn get_num_levels() -> usize {
    4
}

pub(in crate::arch) const fn get_hhdm_base() -> VirtAddr {
    VirtAddr::new(0xFFFF_8000_0000_0000)
}

pub(in crate::arch) const fn get_pfndb_base() -> VirtAddr {
    VirtAddr::new(0xFFFF_A000_0000_0000)
}

pub(in crate::arch) const fn get_map_base() -> VirtAddr {
    VirtAddr::new(0xFFFF_C000_0000_0000)
}

pub(in crate::arch) const fn is_user_addr(addr: VirtAddr) -> bool {
    addr.value() < 0x0000_8000_0000_0000
}

#[unsafe(naked)]
pub(in crate::arch) unsafe extern "C" fn copy_from_user(
    dest: *mut u8,
    src: VirtAddr,
    len: usize,
    context: *mut *mut UserAccessRegion,
) -> bool {
    unsafe extern "C" {
        unsafe fn copy_from_user_start();
        unsafe fn copy_from_user_end();
        unsafe fn copy_from_user_fault();
    }

    static READ_UAR: UserAccessRegion = UserAccessRegion {
        start_ip: &(copy_from_user_start as _),
        end_ip: &(copy_from_user_end as _),
        fault_ip: &(copy_from_user_fault as _),
    };

    naked_asm!("
        xchg rcx, rdx
        mov rax, [rip + {uar}]
        mov [rdx], rax

    .global {start}
    {start}:
        rep movsb

    .global {end}
    {end}:
        xor rax, rax
        mov [rdx], rax
        mov rax, 1
        ret

    .global {fault}
    {fault}:
        xor rax, rax
        mov [rdx], rax
        ret",
        uar = sym READ_UAR,
        start = sym copy_from_user_start,
        end = sym copy_from_user_end,
        fault = sym copy_from_user_fault,
    );
}

#[unsafe(naked)]
pub(in crate::arch) unsafe extern "C" fn copy_to_user(
    dest: VirtAddr,
    src: *const u8,
    len: usize,
    context: *mut *mut UserAccessRegion,
) -> bool {
    unsafe extern "C" {
        unsafe fn copy_to_user_start();
        unsafe fn copy_to_user_end();
        unsafe fn copy_to_user_fault();
    }

    static WRITE_UAR: UserAccessRegion = UserAccessRegion {
        start_ip: &(copy_to_user_start as _),
        end_ip: &(copy_to_user_end as _),
        fault_ip: &(copy_to_user_fault as _),
    };

    naked_asm!("
        xchg rcx, rdx
        mov rax, [rip + {uar}]
        mov [rdx], rax

    .global {start}
    {start}:
        rep movsb

    .global {end}
    {end}:
        xor rax, rax
        mov [rdx], rax
        mov rax, 1
        ret

    .global {fault}
    {fault}:
        xor rax, rax
        mov [rdx], rax
        ret",
        uar = sym WRITE_UAR,
        start = sym copy_to_user_start,
        end = sym copy_to_user_end,
        fault = sym copy_to_user_fault,
    );
}

#[unsafe(naked)]
pub(in crate::arch) unsafe extern "C" fn cstr_len_user(
    src: VirtAddr,
    max_len: usize,
    count: *mut usize,
    context: *mut *mut UserAccessRegion,
) -> bool {
    unsafe extern "C" {
        unsafe fn cstr_len_user_start();
        unsafe fn cstr_len_user_end();
        unsafe fn cstr_len_user_fault();
    }

    static CSTR_UAR: UserAccessRegion = UserAccessRegion {
        start_ip: &(cstr_len_user_start as _),
        end_ip: &(cstr_len_user_end as _),
        fault_ip: &(cstr_len_user_fault as _),
    };

    naked_asm!("
        mov rax, [rip + {uar}]
        mov [rcx], rax

    .global {start}
    {start}:
        xor r8, r8
    .Lloop:
        cmp byte ptr [rdi + r8], 0
        je .Lleave
        inc r8
        cmp rsi, r8
        jne .Lloop
    .Lleave:
        mov [rdx], r8

    .global {end}
    {end}:
        xor rax, rax
        mov [rcx], rax
        mov rax, 1
        ret

    .global {fault}
    {fault}:
        xor rax, rax
        mov [rcx], rax
        ret",
        uar = sym CSTR_UAR,
        start = sym cstr_len_user_start,
        end = sym cstr_len_user_end,
        fault = sym cstr_len_user_fault,
    );
}

pub(in crate::arch) unsafe fn set_user_access(state: bool) {
    if *ARCH_DATA.get().can_smap.get() {
        if state == true {
            unsafe { asm!("stac") };
        } else {
            unsafe { asm!("clac") };
        }
    }
}
