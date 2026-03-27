use alloc::boxed::Box;

use super::{
    ARCH_DATA,
    asm::{rdmsr, wrmsr},
    consts::{self},
    cpu::get_per_cpu,
    system::{apic, gdt::Gdt},
    system::{apic::LAPIC, gdt::TSS},
};
use crate::{
    arch,
    irq::lock::{IrqGuard, IrqLock},
    memory::{VirtAddr, virt::KERNEL_STACK_SIZE},
    percpu::CpuData,
    posix::errno::EResult,
    process::task::Task,
    sched::Scheduler,
};
use core::{
    arch::{asm, naked_asm},
    fmt::Write,
    mem::offset_of,
    sync::atomic::Ordering,
};

#[repr(C)]
#[derive(Default, Debug, Clone)]
pub struct TaskContext {
    pub rsp: u64,
    pub fpu_region: Box<[u8]>,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub fsbase: u64,
    pub gsbase: u64,
    pub restarted: bool,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Context {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    // Pushed onto the stack by the interrupt handler stubs.
    pub isr: u64,
    // Pushed onto the stack by the CPU if the interrupt has an error code.
    pub error: u64,
    // The rest is pushed onto the stack by the CPU during an interrupt.
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}
static_assert!(size_of::<Context>() == 22 * size_of::<u64>());

impl Context {
    pub fn set_return(&mut self, val: usize, err: usize) {
        self.rax = val as _;
        self.rdx = err as _;
    }
}

impl core::fmt::Debug for Context {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_char('\n')?;
        f.write_fmt(format_args!(
            "rax {:016x} rbx {:016x} rcx {:016x} rdx {:016x}\n",
            self.rax, self.rbx, self.rcx, self.rdx
        ))?;
        f.write_fmt(format_args!(
            "rbp {:016x} rdi {:016x} rsi {:016x} r8  {:016x}\n",
            self.rbp, self.rdi, self.rsi, self.r8
        ))?;
        f.write_fmt(format_args!(
            "r9  {:016x} r10 {:016x} r11 {:016x} r12 {:016x}\n",
            self.r9, self.r10, self.r11, self.r12,
        ))?;
        f.write_fmt(format_args!(
            "r13 {:016x} r14 {:016x} r15 {:016x} rfl {:016x}\n",
            self.r13, self.r14, self.r15, self.rflags
        ))?;
        f.write_fmt(format_args!(
            "rsp {:016x} rip {:016x} cs  {:016x} ss  {:016x}",
            self.rsp, self.rip, self.cs, self.ss
        ))?;
        Ok(())
    }
}

/// The task frame consists of registers that the C ABI marks as callee-saved.
/// If we don't save them, these registers are lost during a context switch.
/// The order of these fields is important.
#[repr(C)]
struct TaskFrame {
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
}

pub(in crate::arch) unsafe fn switch(from: *const Task, to: *const Task, irq_guard: IrqGuard) {
    unsafe {
        let from = from.as_ref().unwrap();
        let to = to.as_ref().unwrap();

        let mut from_context = from.task_context.lock();
        let to_context = to.task_context.lock();

        let cpu = ARCH_DATA.get();
        TSS.get().lock().rsp0 = (to.kernel_stack.load(Ordering::Relaxed) + KERNEL_STACK_SIZE) as _;

        if from.is_user() {
            cpu.fpu_save.get()(from_context.fpu_region.as_mut_ptr());
            from_context.ds = super::asm::read_ds();
            from_context.es = super::asm::read_es();
            from_context.fs = super::asm::read_fs();
            from_context.gs = super::asm::read_gs();
            from_context.fsbase = rdmsr(consts::MSR_FS_BASE);
            from_context.gsbase = rdmsr(consts::MSR_KERNEL_GS_BASE);
        }

        if to.is_user() {
            cpu.fpu_restore.get()(to_context.fpu_region.as_ptr());
            super::asm::write_ds(to_context.ds);
            super::asm::write_es(to_context.es);
            super::asm::write_fs(to_context.fs);

            // If we have to change the GS segment we need to reload the MSR, otherwise we lose its value.
            if to_context.gs != super::asm::read_gs() {
                let percpu = get_per_cpu();
                super::asm::write_gs(to_context.gs);
                wrmsr(consts::MSR_GS_BASE, percpu as u64);
            }
            wrmsr(consts::MSR_FS_BASE, to_context.fsbase);
            // KERNEL_GS_BASE is the inactive base (swapped to during iretq/sysretq).
            wrmsr(consts::MSR_KERNEL_GS_BASE, to_context.gsbase);
        }

        let old_rsp = &raw mut from_context.rsp;
        let new_rsp = to_context.rsp;

        drop(from_context);
        drop(to_context);
        drop(irq_guard);
        perform_switch(old_rsp, new_rsp);
    }
}

#[unsafe(naked)]
unsafe extern "C" fn perform_switch(old_rsp: *mut u64, new_rsp: u64) {
    naked_asm!(
        "sub rsp, 0x30", // Make room for all regs (except RIP).
        "mov [rsp + {rbx}], rbx",
        "mov [rsp + {rbp}], rbp",
        "mov [rsp + {r12}], r12",
        "mov [rsp + {r13}], r13",
        "mov [rsp + {r14}], r14",
        "mov [rsp + {r15}], r15",
        "mov [rdi], rsp", // rdi = old_rsp
        "mov rsp, rsi", // rsi = new_rsp
        "mov rbx, [rsp + {rbx}]",
        "mov rbp, [rsp + {rbp}]",
        "mov r12, [rsp + {r12}]",
        "mov r13, [rsp + {r13}]",
        "mov r14, [rsp + {r14}]",
        "mov r15, [rsp + {r15}]",
        "add rsp, 0x30",
        "ret", // This will conveniently move us to the RIP we put at this stack entry.
        rbx = const offset_of!(TaskFrame, rbx),
        rbp = const offset_of!(TaskFrame, rbp),
        r12 = const offset_of!(TaskFrame, r12),
        r13 = const offset_of!(TaskFrame, r13),
        r14 = const offset_of!(TaskFrame, r14),
        r15 = const offset_of!(TaskFrame, r15),
    );
}

pub(in crate::arch) fn init_task(
    context: &mut TaskContext,
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
    stack_start: VirtAddr,
    is_user: bool,
) -> EResult<()> {
    let cpu = ARCH_DATA.get();
    // Prepare a dummy stack with an entry point function to return to.
    unsafe {
        let frame = ((stack_start.value() + KERNEL_STACK_SIZE) as *mut TaskFrame).sub(1);
        (*frame).rbx = entry as *const () as u64;
        (*frame).r12 = arg1 as u64;
        (*frame).r13 = arg2 as u64;
        (*frame).rip = task_entry_thunk as *const () as u64;
        context.rsp = frame as u64;

        if is_user {
            context.fpu_region = vec![0u8; *cpu.fpu_size.get()].into_boxed_slice();
            cpu.fpu_save.get()(context.fpu_region.as_mut_ptr());

            context.ds = super::asm::read_ds();
            context.es = super::asm::read_es();
            context.fs = super::asm::read_fs();
            context.gs = super::asm::read_gs();
            context.fsbase = super::asm::rdmsr(consts::MSR_FS_BASE);
            context.gsbase = super::asm::rdmsr(consts::MSR_KERNEL_GS_BASE);
        }
    }

    Ok(())
}

/// This function only calls [`crate::sched::task_entry`] by moving values from callee saved regs to use the C ABI.
#[unsafe(naked)]
unsafe extern "C" fn task_entry_thunk() -> ! {
    naked_asm!(
        "mov rdi, rbx",
        "mov rsi, r12",
        "mov rdx, r13",
        "push 0", // Make sure to zero this so stack tracing stops here.
        "jmp {task_thunk}",
        task_thunk = sym crate::sched::task_entry,
    );
}

#[inline]
pub(in crate::arch) unsafe fn preempt_disable() {
    unsafe {
        asm!("inc qword ptr gs:{offset}", offset = const offset_of!(CpuData, scheduler.preempt_level), options(nostack));
    }
}

#[inline]
pub(in crate::arch) unsafe fn preempt_enable() -> bool {
    let mut r = false;
    unsafe {
        asm!(
            "dec qword ptr gs:{offset}",
            "jz {label}",
            label = label {
                r = true;
            },
            offset = const offset_of!(CpuData, scheduler.preempt_level),
            options(nostack));
    }
    return r;
}

pub unsafe fn remote_reschedule(cpu: u32) {
    let lapic = LAPIC.get();
    lapic.send_ipi(
        apic::IpiTarget::Specific(cpu),
        consts::IDT_IPI_RESCHED,
        apic::DeliveryMode::Fixed,
        apic::DestinationMode::Logical,
        apic::DeliveryStatus::Pending,
        apic::Level::Assert,
        apic::TriggerMode::Edge,
    );
}

pub(in crate::arch) unsafe fn jump_to_user(ip: VirtAddr, sp: VirtAddr) -> ! {
    assert!(
        Scheduler::get_current().is_user(),
        "Attempted to perform a user jump on a kernel task!"
    );

    // Create a new context for the user jump.
    let mut context = Context {
        rip: ip.value() as u64,
        rsp: sp.value() as u64,
        rflags: 0x202,
        cs: offset_of!(Gdt, user_code64) as u64 | consts::CPL_USER as u64,
        ss: offset_of!(Gdt, user_data) as u64 | consts::CPL_USER as u64,
        ..Context::default()
    };

    // Clear segment registers. Because this also clears GSBASE, we have to restore it immediately.
    unsafe {
        let lock = IrqLock::lock();
        let percpu = get_per_cpu();

        let zero = 0u16;
        asm!("mov ds, {zero:x}", "mov es, {zero:x}", "mov fs, {zero:x}", "mov gs, {zero:x}", zero = in(reg) zero);

        wrmsr(consts::MSR_FS_BASE, 0);
        wrmsr(consts::MSR_GS_BASE, percpu as u64);
        wrmsr(consts::MSR_KERNEL_GS_BASE, 0);

        drop(lock);
        jump_to_context(&raw mut context);
    }
}

pub(in crate::arch) unsafe fn jump_to_context(context: *mut Context) -> ! {
    unsafe {
        asm!(
            "mov rsp, {context}",
            "jmp {interrupt_return}",
            context = in(reg) context,
            interrupt_return = sym arch::x86_64::irq::interrupt_return
        );

        unreachable!();
    }
}
