use crate::{
    arch::x86_64::{
        asm, consts,
        irq::idt_handler,
        sched::Context,
        system::gdt::{Gdt, TSS},
    },
    percpu::CpuData,
    process::signal,
};
use core::{
    arch::{asm, naked_asm},
    mem::offset_of,
    sync::atomic::{AtomicBool, Ordering},
};
use num_enum::TryFromPrimitive;

per_cpu! {
    static ENABLED: AtomicBool = AtomicBool::new(false);
}

#[inline]
pub fn enabled() -> bool {
    ENABLED.get().load(Ordering::Relaxed)
}

#[inline]
pub fn set_rsp0(rsp0: u64) {
    unsafe {
        asm::wrmsr(consts::MSR_FRED_RSP0, rsp0);
    }
}

#[repr(u8)]
#[derive(TryFromPrimitive)]
enum EventType {
    ExternalInterrupt = 0,
    Nmi = 2,
    HardwareException = 3,
    SoftwareInterrupt = 4,
    PrivilegedSoftwareException = 5,
    SoftwareException = 6,
    Syscall = 7,
}

pub unsafe extern "C" fn dispatch(frame: *mut Context) {
    let ctx = unsafe { frame.as_mut().unwrap() };
    let vector = (ctx.ss >> 32) & 0xFF;
    let from_user = (ctx.cs & 3) == 3;

    let event_type = match EventType::try_from(((ctx.ss >> 48) & 0xF) as u8) {
        Ok(event_type) => event_type,
        Err(_) => {
            panic!("unexpected event type");
        }
    };

    match event_type {
        EventType::Syscall => {
            if from_user {
                let restart = ctx.snapshot_syscall();
                crate::syscall::dispatch(ctx);
                signal::deliver_pending_signals(ctx, Some(restart));
            } else {
                panic!("unexpected syscall from kernel space");
            }
        }
        EventType::Nmi | EventType::HardwareException | EventType::ExternalInterrupt => {
            ctx.isr = vector;
            unsafe { idt_handler(ctx) };
        }
        EventType::PrivilegedSoftwareException => {
            panic!("unexpected privileged software exception");
        }
        _ => {}
    }
}

pub fn check() -> bool {
    // Ensure leaf 7 is supported.
    let mut cpuid = asm::cpuid(0, 0);
    if cpuid.eax < 7 {
        return false;
    }

    // Ensure subleaf 1 is supported.
    cpuid = asm::cpuid(7, 0);
    if cpuid.eax < 1 {
        return false;
    }

    // Fred supprt is enumerated by leaf 7 subleaf 1 bit 17.
    cpuid = asm::cpuid(7, 1);
    if cpuid.eax & (1 << 17) == 0 {
        return false;
    }

    ENABLED.get().store(true, Ordering::Relaxed);
    true
}

pub fn init() {
    log!("Enabling FRED on core {}", CpuData::get().id);

    let addr = fred_ring3_entry as *const () as u64;
    assert!(addr % 4096 == 0);

    unsafe {
        asm::wrmsr(consts::MSR_FRED_CONFIG, addr);
        asm::wrmsr(consts::MSR_FRED_STKLVLS, 0);
    }

    let mut cr4: usize;
    unsafe {
        asm!("mov {cr4}, cr4", cr4 = out(reg) cr4, options(nostack));
    }
    cr4 |= consts::CR4_FRED;
    unsafe {
        asm!("mov cr4, {cr4}", cr4 = in(reg) cr4, options(nostack));
    }

    let rsp0 = TSS.get().lock().rsp0;
    set_rsp0(rsp0);
}

const CS_OFFSET: usize = size_of::<Context>() - size_of::<u64>() - offset_of!(Context, cs);

#[unsafe(naked)]
pub unsafe extern "C" fn interrupt_return() {
    naked_asm!(
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rsi",
        "pop rdi",
        "pop rbp",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        // Use erets if we came from the kernel.
        "cmp word ptr [rsp+{cs}], {kernel_cs}",
        "je kernel",
        // Skip the .isr field.
        "add rsp, 0x8",
        "eretu",
        "kernel:",
        // Skip the .isr field.
        "add rsp, 0x8",
        "erets",
        cs = const CS_OFFSET,
        kernel_cs = const offset_of!(Gdt, kernel64_code),
    );
}

#[unsafe(naked)]
#[rustc_align(4096)] // The entry must be aligned to a page.
pub unsafe extern "C" fn fred_ring3_entry() {
    naked_asm!(
        "fred_ring3_entry_asm:",
        "push 3",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rbp",
        "push rdi",
        "push rsi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Zero out the base pointer since we can't trust it.
        "xor ebp, ebp",
        "mov rdi, rsp",
        "cld",
        "call {dispatch}",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rsi",
        "pop rdi",
        "pop rbp",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "add rsp, 8",
        "eretu",

        // The ring0 entry must be 256 bytes from the start of the page.
        ".fill 256 - (. - fred_ring3_entry_asm), 1, 0",

        "fred_ring0_entry_asm:",
        "push 0",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rbp",
        "push rdi",
        "push rsi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Zero out the base pointer since we can't trust it.
        "xor ebp, ebp",
        "mov rdi, rsp",
        "cld",
        "call {dispatch}",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rsi",
        "pop rdi",
        "pop rbp",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "add rsp, 8",
        "erets",
        dispatch = sym dispatch,
    )
}
