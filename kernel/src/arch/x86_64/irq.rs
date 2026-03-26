use super::{consts, sched::Context};
use crate::{
    arch::x86_64::{
        consts::CPL_USER,
        cpu::halt,
        system::{apic::LAPIC, gdt::Gdt},
    },
    irq::{IrqLine, lock::IrqLock},
    memory::fault::PageFaultInfo,
    percpu::CpuData,
    sched::Scheduler,
    util::mutex::spin::SpinMutex,
};
use alloc::sync::Arc;
use core::{
    arch::{asm, naked_asm},
    mem::offset_of,
    sync::atomic::Ordering,
};
use seq_macro::seq;

pub(in crate::arch) unsafe fn set_irq_state(value: bool) -> bool {
    let old_mask = get_irq_state();
    unsafe {
        if value {
            asm!("sti", options(nostack));
        } else {
            asm!("cli", options(nostack));
        }
    }
    return old_mask;
}

pub(in crate::arch) fn get_irq_state() -> bool {
    let mut flags: u64;
    unsafe {
        asm!("pushf; pop {0}", out(reg) flags);
    }
    return flags & (consts::RFLAGS_IF as u64) != 0;
}

pub(in crate::arch) fn wait_for_irq() {
    unsafe {
        asm!("hlt", options(nostack));
    }
}

/// Handles a syscall via AMD64 syscall/sysret instructions.
/// # Safety
/// Assumes that a valid stack is ready in the PerCpu block at this point.
#[unsafe(naked)]
pub unsafe extern "C" fn amd64_syscall_stub() {
    naked_asm!(
        "swapgs",
        "mov gs:{user_stack}, rsp",
        "mov rsp, gs:{kernel_stack}",
        "cld",
        // We're pretending to be an interrupt, so fill the bottom fields of `Context`.
        "push {user_data}",           // SS and CS are not changed during SYSCALL. Use `Gdt::user_data | CPL_USER`.
        "push gs:{user_stack}",
        "push r11",                   // RFLAGS is moved into r11 by the CPU.
        "push {user_code64}",         // Same as SS. Use `Gdt::user_code64 | CPL_USER`
        "push rcx",                   // RIP is moved into rcx by the CPU.
        "push 0x00",                  // Context::error field
        "push 0x00",                  // Context::isr field
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
        "xor rbp, rbp",
        "mov rdi, rsp",               // Put the trap frame struct as first argument.
        "call {syscall_handler}",     // Call syscall handler
        "cli",
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
        "add rsp, 0x10",              // Skip .error and .isr fields (2 * sizeof(u64))
        "mov rsp, gs:{user_stack}",   // Load user stack from `Cpu.user_stack`.
        "swapgs",
        "sysretq",                    // Return to user mode.

        syscall_handler = sym syscall_handler,
        user_stack = const offset_of!(CpuData, user_stack),
        kernel_stack = const offset_of!(CpuData, kernel_stack),
        user_code64 = const offset_of!(Gdt, user_code64) | CPL_USER as usize,
        user_data = const offset_of!(Gdt, user_data) | CPL_USER as usize,
    );
}

/// Invoked by either the interrupt or syscall stub.
extern "C" fn syscall_handler(frame: *mut Context) {
    unsafe {
        let frame = frame.as_mut().unwrap();

        // Arguments use the SYSV C ABI.
        // Except for a3, since RCX is needed for sysret, we need a different register.
        let result = crate::syscall::dispatch(
            frame,
            frame.rax as usize,
            frame.rdi as usize,
            frame.rsi as usize,
            frame.rdx as usize,
            frame.r10 as usize,
            frame.r8 as usize,
            frame.r9 as usize,
        );
        frame.rax = result.0 as u64;
        frame.rdx = result.1 as u64;
    }
}

// There are some interrupts which generate an error code on the stack, while others do not.
// We normalize this by just pushing 0 for those that don't generate an error code.
seq! { N in 0..256 {
    #[unsafe(naked)]
    pub (super) unsafe extern "C" fn interrupt_stub~N() {
        naked_asm!(
            // These codes push an error on the stack. Do nothing.
            ".if ({i} == 8 || ({i} >= 10 && {i} <= 14) || {i} == 17 || {i} == 21 || {i} == 29 || {i} == 30)",
            // All other ones don't, so we need to push something ourselves.
            ".else",
            "push 0",
            ".endif",

            "push {i}",
            "jmp {interrupt_stub_internal}",

            i = const N,
            interrupt_stub_internal = sym interrupt_stub_internal
        );
    }
}}

const CS_OFFSET: usize = size_of::<Context>() - size_of::<u64>() - offset_of!(Context, cs);

/// To avoid having 256 big functions with essentially the same logic,
/// this function is meant to do the actual heavy lifting.
#[unsafe(naked)]
unsafe extern "C" fn interrupt_stub_internal() {
    naked_asm!(
        // Load the kernel GS base if we're coming from user space.
        "cmp word ptr [rsp+{cs}], {kernel_cs}",
        "je 2f",
        "swapgs",
        "2:",
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
        "cld",
        // Zero out the base pointer since we can't trust it.
        "xor rbp, rbp",
        // Load the frame as first argument.
        "mov rdi, rsp",
        "call {interrupt_handler}",
        "jmp {interrupt_return}",
        cs = const CS_OFFSET,
        kernel_cs = const offset_of!(Gdt, kernel64_code),
        interrupt_handler = sym idt_handler,
        interrupt_return = sym interrupt_return
    );
}

/// Returns from an interrupt frame.
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
        // Change GS back if we came from user mode.
        "cmp word ptr [rsp+{cs}], {kernel_cs}",
        "je 2f",
        "swapgs",
        "2:",
        // Skip .error and .isr fields.
        "add rsp, 0x10",
        "iretq",
        cs = const CS_OFFSET,
        kernel_cs = const offset_of!(Gdt, kernel64_code),
    );
}

per_cpu! {
    /// On x86_64, interrupts don't go through the interrupt controller, but map directly to IDT vectors.
    /// To emulate the concept of interrupt lines, we define an array of [`IrqLine`]s.
    pub static IRQ_LINES: SpinMutex<[Option<Arc<dyn IrqLine>>; 128]> = SpinMutex::new([const { None }; _]);
}

/// Invoked by an interrupt stub.
unsafe extern "C" fn idt_handler(context: *mut Context) {
    let old = IrqLock::set_interrupted(true);
    let context = unsafe { context.as_mut().unwrap() };
    let isr = context.isr;

    match isr as u8 {
        // Exceptions.
        consts::IDT_PF => {
            page_fault_handler(context);
        }
        // Unhandled exceptions.
        0x00..0x20 => {
            error!("{:?}", context);
            panic!("Got an exception {}", isr);
        }
        // IPIs
        consts::IDT_IPI_PANIC => {
            halt();
        }
        consts::IDT_IPI_RESCHED => {
            unsafe { crate::arch::sched::preempt_disable() };
            if unsafe { crate::arch::sched::preempt_enable() } {
                LAPIC.get().eoi();
                CpuData::get().scheduler.reschedule();
            }
        }
        // Any other ISR is an IRQ with a dynamic handler.
        _ => {
            match &mut IRQ_LINES.get().lock()[isr as usize - 0x20] {
                Some(x) => x.raise(),
                None => panic!("Unhandled interrupt on ISR {}", isr),
            };
        }
    }

    IrqLock::set_interrupted(old);
}

// /// Try to send a signal to the user-space program or panic if the interrupt is caused by the kernel.
// fn try_signal_or_die(context: &Context, signal: u32, code: u32) {}

fn page_fault_handler(context: &mut Context) {
    let mut cr2: usize;
    unsafe { asm!("mov {cr2}, cr2", cr2 = out(reg) cr2) };

    let err = context.error;
    let info = PageFaultInfo {
        page_was_present: err & (1 << 0) != 0,
        caused_by_write: err & (1 << 1) != 0,
        caused_by_fetch: err & (1 << 4) != 0,
        caused_by_user: context.cs & consts::CPL_USER as u64 == consts::CPL_USER as u64,
        ip: (context.rip as usize).into(),
        addr: cr2.into(),
    };

    if !crate::memory::virt::fault::handler(&info) {
        let task = Scheduler::get_current();
        let uar = task.uar.load(Ordering::Relaxed);
        context.rip = unsafe { (*uar).fault_ip } as *const _ as u64;
    }
}
