use crate::{
    arch::x86_64::{
        ARCH_DATA,
        asm::wrmsr,
        consts::{self, MSR_FS_BASE},
        irq,
        system::{
            apic::{self, LAPIC, LocalApic},
            gdt, idt,
        },
    },
    clock,
    percpu::{CpuData, LD_PERCPU_START},
    posix::errno::{EResult, Errno},
    uapi,
};
use core::{
    arch::{asm, naked_asm},
    mem::offset_of,
    ptr::null_mut,
    sync::atomic::{AtomicBool, Ordering},
};

unsafe extern "C" {
    unsafe static LD_STACK_TOP: u8;
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn _start() {
    naked_asm!(
        "lea rsp, [rip + {stack}]",
        "jmp {entry}",
        stack = sym LD_STACK_TOP,
        entry = sym crate::boot::entry
    );
}

pub(in crate::arch) fn get_frame_pointer() -> usize {
    let mut fp: usize;
    unsafe {
        asm!("mov {fp}, rbp", fp = out(reg) fp, options(nostack));
    }
    return fp;
}

pub(in crate::arch) fn get_per_cpu() -> *mut CpuData {
    unsafe {
        let cpu: *mut CpuData;
        asm!(
            "mov {cpu}, gs:[{this}]",
            cpu = out(reg) cpu,
            this = const offset_of!(CpuData, this),
            options(nostack, preserves_flags),
        );
        assert_ne!(cpu, null_mut());
        return cpu;
    }
}

pub fn setup_bsp() {
    apic::disable_legacy_pic();
    idt::init();
    idt::set_idt();

    // Set FSGSBASE contents.
    unsafe {
        super::asm::wrmsr(consts::MSR_KERNEL_GS_BASE, 0);
        super::asm::wrmsr(consts::MSR_GS_BASE, &raw const LD_PERCPU_START as u64);
        super::asm::wrmsr(consts::MSR_FS_BASE, 0);
    }

    CpuData::get().present.store(true, Ordering::Relaxed);
}

pub(super) fn setup_core(context: &'static CpuData) {
    let mut cr0: usize;
    let mut cr4: usize;

    unsafe {
        // Set FSGSBASE contents.
        // Slightly misleading, but KERNEL_GS_BASE is the currently inactive GSBASE value.
        super::asm::wrmsr(consts::MSR_KERNEL_GS_BASE, 0);
        // We will save a reference to this struct in GS_BASE.
        super::asm::wrmsr(
            consts::MSR_GS_BASE,
            context.this.load(Ordering::Acquire) as u64,
        );
        super::asm::wrmsr(consts::MSR_FS_BASE, 0);
    }

    // Load a GDT and TSS.
    gdt::init();

    // Load the IDT.
    // Note: The IDT itself is global, but still needs to be loaded for each CPU.
    idt::set_idt();

    // Enable the `syscall` extension.
    unsafe {
        let msr = super::asm::rdmsr(consts::MSR_EFER);
        super::asm::wrmsr(consts::MSR_EFER, msr | consts::MSR_EFER_SCE as u64);
        // Bits 32-47 are kernel segment base, Bits 48-63 are user segment base. Lower 32 bits (EIP) are unused.
        super::asm::wrmsr(
            consts::MSR_STAR,
            ((offset_of!(gdt::Gdt, user_code) | consts::CPL_USER as usize) as u64) << 48
                | (offset_of!(gdt::Gdt, kernel64_code) as u64) << 32,
        );
        // Set syscall entry point.
        super::asm::wrmsr(
            consts::MSR_LSTAR,
            irq::amd64_syscall_stub as *const () as u64,
        );
        super::asm::wrmsr(
            consts::MSR_SFMASK,
            (consts::RFLAGS_AC | consts::RFLAGS_DF | consts::RFLAGS_IF) as u64,
        );

        asm!("mov {cr0}, cr0", cr0 = out(reg) cr0, options(nostack));
        asm!("mov {cr4}, cr4", cr4 = out(reg) cr4, options(nostack));
    }

    // Collect all relevant CPUIDs.
    let (cpuid1, cpuid7, cpuid13) = (
        super::asm::cpuid(1, 0),
        super::asm::cpuid(7, 0),
        super::asm::cpuid(13, 0),
    );

    // Enable SSE.
    cr0 &= !consts::CR0_EM; // Clear EM bit.
    cr0 |= consts::CR0_MP;
    cr4 |= consts::CR4_OSFXSR | consts::CR4_OSXMMEXCPT;

    let cpu = ARCH_DATA.get();

    // XSAVE
    if cpuid1.ecx & consts::CPUID_1C_XSAVE != 0 {
        cr4 |= consts::CR4_OSXSAVE;
        unsafe { asm!("mov cr4, {cr4}", cr4 = in(reg) cr4, options(nostack)) };
        let mut xcr0 = 0u64;
        xcr0 |= 3;

        // AVX
        if cpuid1.ecx & consts::CPUID_1C_AVX != 0 {
            xcr0 |= 1 << 2;
        }

        // AVX-512
        if cpuid7.ebx & consts::CPUID_7B_AVX512F != 0 {
            xcr0 |= 1 << 5;
            xcr0 |= 1 << 6;
            xcr0 |= 1 << 7;
        }

        unsafe {
            super::asm::wrxcr(0, xcr0);

            // Change callbacks from FXSAVE to XSAVE.
            cpu.fpu_size.init(cpuid13.ecx as usize); // ECX contains the size of the FPU block to save.
            cpu.fpu_save.init(super::asm::xsave);
            cpu.fpu_restore.init(super::asm::xrstor);
        }
    } else {
        unsafe {
            cpu.fpu_size.init(512);
            cpu.fpu_save.init(super::asm::fxsave);
            cpu.fpu_restore.init(super::asm::fxrstor);
        }
    }

    // TODO
    if cpuid7.ecx & consts::CPUID_7C_UMIP != 0 {
        cr4 |= consts::CR4_UMIP;
    }

    if cpuid7.ebx & consts::CPUID_7B_SMEP != 0 {
        cr4 |= consts::CR4_SMEP;
    }

    unsafe { cpu.can_smap.init(cpuid7.ebx & consts::CPUID_7B_SMAP != 0) };
    if *cpu.can_smap.get() {
        cr4 |= consts::CR4_SMAP;
    }

    if cpuid7.ebx & consts::CPUID_7B_FSGSBASE != 0 {
        cr4 |= consts::CR4_FSGSBASE;
    }

    unsafe {
        // Write back the modified control register values.
        asm!("mov cr0, {cr0}", cr0 = in(reg) cr0, options(nostack));
        asm!("mov cr4, {cr4}", cr4 = in(reg) cr4, options(nostack));
    }

    LocalApic::init();

    context.present.store(true, Ordering::Release);
    context.online.store(true, Ordering::Release);
}

pub static IS_INIT: AtomicBool = AtomicBool::new(false);

pub(in crate::arch) fn halt_others() {
    if !IS_INIT.load(Ordering::Relaxed) {
        return;
    }

    LAPIC.get().send_ipi(
        apic::IpiTarget::AllButThisCpu,
        consts::IDT_IPI_PANIC,
        apic::DeliveryMode::Fixed,
        apic::DestinationMode::Logical,
        apic::DeliveryStatus::Idle,
        apic::Level::Assert,
        apic::TriggerMode::Edge,
    );

    clock::block_ns(10000000).unwrap();
}

pub(in crate::arch) fn halt() -> ! {
    loop {
        unsafe {
            core::arch::asm!("cli; hlt");
        }
    }
}

pub(in crate::arch) fn archctl(cmd: usize, arg: usize) -> EResult<usize> {
    const SET_FS_BASE: usize = uapi::archctl::SET_FSBASE as _;

    match cmd {
        SET_FS_BASE => unsafe { wrmsr(MSR_FS_BASE, arg as u64) },
        _ => return Err(Errno::ENOSYS),
    }

    Ok(0)
}
