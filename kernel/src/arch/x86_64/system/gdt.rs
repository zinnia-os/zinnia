use crate::{
    arch::x86_64::consts::{CPL_KERNEL, CPL_USER, MSR_GS_BASE},
    memory::stack::KernelStack,
    util::{mutex::spin::SpinMutex, once::Once},
};
use bitflags::bitflags;
use bytemuck::{Pod, Zeroable};
use core::{arch::asm, mem::offset_of};

/// Global Descriptor Table.
/// These entries are ordered exactly like this because the SYSRET instruction expects it.
#[repr(C, packed)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct Gdt {
    /// Unused
    pub null: GdtDesc,
    /// 32-bit kernel CS (used for AP bring-up)
    pub kernel32_code: GdtDesc,
    /// 32-bit kernel DS (used for AP bring-up)
    pub kernel32_data: GdtDesc,
    /// 64-bit kernel CS
    pub kernel64_code: GdtDesc,
    /// 64-bit kernel DS
    pub kernel64_data: GdtDesc,
    /// 32-bit compatibility mode user CS (unused)
    pub user_code: GdtDesc,
    /// User DS
    pub user_data: GdtDesc,
    /// 64-bit user CS
    pub user_code64: GdtDesc,
    /// Task state segment
    pub tss: GdtLongDesc,
}

impl Default for Gdt {
    fn default() -> Self {
        Self::new()
    }
}

impl Gdt {
    pub const fn new() -> Self {
        Self {
            null: GdtDesc::empty(),
            kernel32_code: GdtDesc::empty(),
            kernel32_data: GdtDesc::empty(),
            kernel64_code: GdtDesc::empty(),
            kernel64_data: GdtDesc::empty(),
            user_code: GdtDesc::empty(),
            user_data: GdtDesc::empty(),
            user_code64: GdtDesc::empty(),
            tss: GdtLongDesc::empty(),
        }
    }
}

bitflags! {
    pub struct GdtAccess: u8 {
        const None = 0;
        const Present = 1 << 7;
        const Kernel = CPL_KERNEL << 5;
        const User = CPL_USER << 5;
        const Segment = 1 << 4;
        const Executable = 1 << 3;
        const ReadWrite = 1 << 1;
        const Accessed = 1 << 0;
    }

    pub struct GdtFlags: u8 {
        const None = 0;
        const Granularity = 1 << 3;
        const ProtMode = 1 << 2;
        const LongMode = 1 << 1;
    }
}

/// GDT segment descriptor
#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct GdtDesc {
    /// Limit[0..15]
    limit0: u16,
    /// Base[0..15]
    base0: u16,
    /// Base[16..23]
    base1: u8,
    /// Access modifider
    access: u8,
    /// Limit[16..19] and Flags
    limit1_flags: u8,
    /// Base[24..31]
    base2: u8,
}

impl GdtDesc {
    pub const fn empty() -> Self {
        Self {
            limit0: 0,
            base0: 0,
            base1: 0,
            access: 0,
            limit1_flags: 0,
            base2: 0,
        }
    }

    /// Encode a new GDT descriptor
    pub const fn new(limit: u32, base: u32, access: GdtAccess, flags: GdtFlags) -> Self {
        Self {
            limit0: limit as u16,
            base0: base as u16,
            base1: (base >> 16) as u8,
            access: access.bits(),
            limit1_flags: ((flags.bits() << 4) & 0xF0) | (((limit >> 16) as u8) & 0x0F),
            base2: (base >> 24) as u8,
        }
    }
}

/// GDT segment descriptor
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
#[repr(C, packed)]
pub struct GdtLongDesc {
    /// Limit[0..15]
    limit0: u16,
    /// Base[0..15]
    base0: u16,
    /// Base[16..23]
    base1: u8,
    /// Access modifider
    access: u8,
    /// Limit[16..19] and Flags
    limit1_flags: u8,
    /// Base[24..31]
    base2: u8,
    /// Base[32..63]
    base3: u32,
    /// Reserved
    reserved: u32,
}

#[repr(u8)]
pub enum GdtLongType {
    TssAvailable = 0x9,
}

impl GdtLongDesc {
    pub const fn empty() -> Self {
        Self {
            limit0: 0,
            base0: 0,
            base1: 0,
            access: 0,
            limit1_flags: 0,
            base2: 0,
            base3: 0,
            reserved: 0,
        }
    }

    /// Encode a new GDT descriptor
    pub const fn new(
        limit: u32,
        base: u64,
        access: GdtAccess,
        long_type: GdtLongType,
        flags: GdtFlags,
    ) -> Self {
        Self {
            limit0: limit as u16,
            base0: base as u16,
            base1: (base >> 16) as u8,
            access: access.bits() | long_type as u8,
            limit1_flags: ((flags.bits() << 4) & 0xF0) | (((limit >> 16) as u8) & 0x0F),
            base2: (base >> 24) as u8,
            base3: (base >> 32) as u32,
            reserved: 0,
        }
    }

    pub const fn set_base(&mut self, base: u64) {
        self.base0 = base as u16;
        self.base1 = (base >> 16) as u8;
        self.base2 = (base >> 24) as u8;
        self.base3 = (base >> 32) as u32;
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Zeroable, Pod)]
pub struct GdtRegister {
    pub limit: u16,
    pub base: u64,
}

use core::mem::size_of;

#[repr(C, packed)]
#[derive(Debug)]
pub struct TaskStateSegment {
    reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    reserved1: u32,
    reserved2: u32,
    pub ist1: u64,
    pub ist2: u64,
    pub ist3: u64,
    pub ist4: u64,
    pub ist5: u64,
    pub ist6: u64,
    pub ist7: u64,
    reserved3: u32,
    reserved4: u32,
    reserved5: u16,
    iopb: u16,
}
static_assert!(size_of::<TaskStateSegment>() == 0x68);

impl Default for TaskStateSegment {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved1: 0,
            reserved2: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            reserved3: 0,
            reserved4: 0,
            reserved5: 0,
            iopb: size_of::<TaskStateSegment>() as u16,
        }
    }
}

pub const BASE_GDT: Gdt = Gdt {
    null: GdtDesc::new(0, 0, GdtAccess::None, GdtFlags::None),
    kernel32_code: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::ReadWrite
            .union(GdtAccess::Executable)
            .union(GdtAccess::Segment)
            .union(GdtAccess::Present),
        GdtFlags::Granularity.union(GdtFlags::ProtMode),
    ),
    kernel32_data: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::Accessed
            .union(GdtAccess::ReadWrite)
            .union(GdtAccess::Segment)
            .union(GdtAccess::Present),
        GdtFlags::Granularity.union(GdtFlags::ProtMode),
    ),
    kernel64_code: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::Present
            .union(GdtAccess::Kernel)
            .union(GdtAccess::Segment)
            .union(GdtAccess::Executable)
            .union(GdtAccess::ReadWrite)
            .union(GdtAccess::Accessed),
        GdtFlags::Granularity.union(GdtFlags::LongMode),
    ),
    kernel64_data: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::Present
            .union(GdtAccess::Kernel)
            .union(GdtAccess::Segment)
            .union(GdtAccess::ReadWrite)
            .union(GdtAccess::Accessed),
        GdtFlags::Granularity.union(GdtFlags::LongMode),
    ),
    user_code: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::Present
            .union(GdtAccess::User)
            .union(GdtAccess::Segment)
            .union(GdtAccess::ReadWrite),
        GdtFlags::Granularity.union(GdtFlags::ProtMode),
    ),
    user_data: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::Present
            .union(GdtAccess::User)
            .union(GdtAccess::Segment)
            .union(GdtAccess::ReadWrite),
        GdtFlags::Granularity.union(GdtFlags::LongMode),
    ),
    user_code64: GdtDesc::new(
        0xFFFFF,
        0,
        GdtAccess::Present
            .union(GdtAccess::User)
            .union(GdtAccess::Segment)
            .union(GdtAccess::Executable)
            .union(GdtAccess::ReadWrite),
        GdtFlags::Granularity.union(GdtFlags::LongMode),
    ),
    tss: GdtLongDesc::new(
        size_of::<TaskStateSegment>() as u32 - 1,
        0,
        GdtAccess::Present.union(GdtAccess::Kernel),
        GdtLongType::TssAvailable,
        GdtFlags::None,
    ),
};

per_cpu! {
    /// Processor local Global Descriptor Table.
    /// The GDT refers to a different TSS every time, so unlike the IDT it has to exist for each processor.
    pub static GDT: SpinMutex<Gdt> = SpinMutex::new(Gdt::new());

    pub static TSS: SpinMutex<TaskStateSegment> = SpinMutex::new(TaskStateSegment::new());

    static TSS_STACK: Once<KernelStack> = Once::new();
}

pub fn init() {
    let mut gdt = GDT.get().lock();
    let mut tss = TSS.get().lock();

    let stack = KernelStack::new().expect("Failed to allocate TSS stack");
    unsafe { TSS_STACK.get().init(stack) };
    tss.rsp0 = TSS_STACK.get().get().top().into();

    *gdt = BASE_GDT;
    gdt.tss.set_base(unsafe { TSS.get().raw_inner() } as u64);

    // Construct a register to hold the GDT base and limit.
    let gdtr = GdtRegister {
        limit: (size_of::<Gdt>() - 1) as u16,
        base: &*gdt as *const _ as u64,
    };

    unsafe {
        // Load the table into the register.
        asm!("lgdt [{0}]", in(reg) &gdtr);

        // Save the contents of MSR_GS_BASE, they get cleared by a write to `gs`.
        let gs = crate::arch::x86_64::asm::rdmsr(MSR_GS_BASE);

        // Flush and reload the segment registers.
        asm!(
            "push {code_seg}",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, {data_seg}",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            code_seg = const offset_of!(Gdt, kernel64_code),
            data_seg = const offset_of!(Gdt, kernel64_data),
            lateout("rax") _ // (R)AX was modified.
        );

        // Restore MSR_GS_BASE
        crate::arch::x86_64::asm::wrmsr(MSR_GS_BASE, gs);

        // Load the TSS.
        asm!(
            "mov ax, {offset}",
            "ltr ax",
            offset = const offset_of!(Gdt, tss) as u16,
            lateout("rax") _
        );
    }
}
