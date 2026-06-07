use super::{
    ARCH_DATA,
    asm::{rdmsr, wrmsr},
    consts::{self},
    cpu::get_per_cpu,
    system::{apic, gdt::Gdt},
    system::{apic::LAPIC, gdt::TSS},
};
use crate::{
    irq::lock::IrqLock,
    memory::{UserPtr, VirtAddr, stack::KernelStack},
    percpu::CpuData,
    posix::errno::EResult,
    process::{
        Process, State,
        signal::{Signal, SignalDelivery, SignalSet},
        task::Task,
    },
    sched::Scheduler,
    uapi::signal::{SA_ONSTACK, siginfo_t},
};
use alloc::boxed::Box;
use core::{
    arch::{asm, naked_asm},
    fmt::Write,
    mem::offset_of,
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
    pub fn syscall_number(&self) -> usize {
        self.rax as usize
    }

    pub fn syscall_error(&self) -> usize {
        self.rdx as usize
    }

    pub fn arg0(&self) -> usize {
        self.rdi as usize
    }

    pub fn arg1(&self) -> usize {
        self.rsi as usize
    }

    pub fn arg2(&self) -> usize {
        self.rdx as usize
    }

    pub fn arg3(&self) -> usize {
        self.r10 as usize
    }

    pub fn arg4(&self) -> usize {
        self.r8 as usize
    }

    pub fn arg5(&self) -> usize {
        self.r9 as usize
    }

    pub fn set_return(&mut self, val: usize, err: usize) {
        self.rax = val as _;
        self.rdx = err as _;
    }

    pub fn sp(&self) -> usize {
        self.rsp as usize
    }

    pub fn ip(&self) -> usize {
        self.rip as usize
    }

    pub fn snapshot_syscall(&self) -> SyscallRestart {
        SyscallRestart {
            nr: self.rax as usize,
            arg2: self.rdx as usize,
        }
    }

    pub fn restart_syscall(&mut self, restart: &SyscallRestart) {
        self.rip -= 2; // Length of the `syscall` instruction.
        self.rax = restart.nr as u64;
        self.rdx = restart.arg2 as u64;
    }
}

/// Registers captured before a syscall is dispatched so that an interrupted
/// syscall can be transparently restarted afterwards.
/// The fields are whichever registers [`Context::set_return`] would clobber.
#[derive(Clone, Copy, Debug)]
pub struct SyscallRestart {
    nr: usize,
    arg2: usize,
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

pub(in crate::arch) unsafe fn switch(from: *const Task, to: *const Task) -> *mut Task {
    unsafe {
        let previous = from as *mut Task;
        let from = from.as_ref().unwrap();
        let to = to.as_ref().unwrap();

        let from_context = &mut *from.task_context.get();
        let to_context = &mut *to.task_context.get();

        let cpu = ARCH_DATA.get();
        TSS.get().lock().rsp0 = to.kernel_stack.top().into();

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
        perform_switch(old_rsp, new_rsp, previous)
    }
}

#[unsafe(naked)]
unsafe extern "C" fn perform_switch(
    old_rsp: *mut u64,
    new_rsp: u64,
    previous: *mut Task,
) -> *mut Task {
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
        "mov rax, rdx",
        "ret", // This will conveniently move us to the RIP we put at this stack entry.
        rbx = const offset_of!(TaskFrame, rbx),
        rbp = const offset_of!(TaskFrame, rbp),
        r12 = const offset_of!(TaskFrame, r12),
        r13 = const offset_of!(TaskFrame, r13),
        r14 = const offset_of!(TaskFrame, r14),
        r15 = const offset_of!(TaskFrame, r15),
    );
}

#[unsafe(naked)]
pub(in crate::arch) extern "C" fn run_on_stack_raw(
    stack_top: usize,
    f: extern "C" fn(usize, usize) -> !,
    arg: usize,
) -> ! {
    naked_asm!(
        "xor ebp, ebp",
        "mov rax, rsp",
        "mov rsp, rdi",
        "mov rdi, rax",
        "mov rax, rsi",
        "mov rsi, rdx",
        "call rax",
        "ud2",
    )
}

pub(in crate::arch) fn init_task(
    context: &mut TaskContext,
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
    stack: &KernelStack,
    is_user: bool,
) -> EResult<()> {
    let cpu = ARCH_DATA.get();
    // Prepare a dummy stack with an entry point function to return to.
    unsafe {
        let frame = stack.top().as_ptr::<TaskFrame>().sub(1);
        frame.write(TaskFrame {
            rbx: entry as *const () as u64,
            rbp: 0,
            r12: arg1 as u64,
            r13: arg2 as u64,
            r14: 0,
            r15: 0,
            rip: task_entry_thunk as *const () as u64,
        });
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
        "mov rdi, rax",
        "mov rsi, rbx",
        "mov rdx, r12",
        "mov rcx, r13",
        "push 0", // Make sure to zero this so stack tracing stops here.
        "jmp {task_thunk}",
        task_thunk = sym crate::sched::task_entry_after_switch,
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
    unsafe { send_ipi_to(cpu, consts::IDT_IPI_RESCHED) };
}

pub fn broadcast_shootdown() {
    LAPIC.get().send_ipi(
        apic::IpiTarget::AllButThisCpu,
        consts::IDT_IPI_SHOOTDOWN,
        apic::DeliveryMode::Fixed,
        apic::DestinationMode::Physical,
        apic::DeliveryStatus::Idle,
        apic::Level::Assert,
        apic::TriggerMode::Edge,
    );
}

unsafe fn send_ipi_to(cpu: u32, vector: u8) {
    let lapic = LAPIC.get();
    let target_lapic_id = LAPIC.get_for(CpuData::get_for(cpu).unwrap()).cached_id();
    lapic.send_ipi(
        apic::IpiTarget::Specific(target_lapic_id),
        vector,
        apic::DeliveryMode::Fixed,
        apic::DestinationMode::Physical,
        apic::DeliveryStatus::Idle,
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
            interrupt_return = sym crate::arch::x86_64::irq::interrupt_return
        );

        unreachable!();
    }
}

/// Saved state for `sigreturn`, written to the user stack below the rest of the signal frame.
#[repr(C)]
#[derive(Clone, Copy)]
struct SignalFrame {
    saved_mask: u64,
    saved_context: Context,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserStack {
    ss_sp: u64,
    ss_size: u64,
    ss_flags: i32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Mcontext {
    oldmask: u64,
    gregs: [u64; 16],
    pc: u64,
    pr: u64,
    sr: u64,
    gbr: u64,
    mach: u64,
    macl: u64,
    fpregs: [u64; 16],
    xfpregs: [u64; 16],
    fpscr: u32,
    fpul: u32,
    ownedfp: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Ucontext {
    uc_link: u64,
    uc_stack: UserStack,
    uc_mcontext: Mcontext,
    uc_sigmask: u64,
}

pub(in crate::arch) fn setup_signal_frame(context: &mut Context, delivery: &SignalDelivery) {
    let altstack = delivery.altstack;
    let on_altstack = altstack.contains(context.rsp as usize);
    let use_altstack = delivery.flags & SA_ONSTACK != 0 && altstack.is_enabled() && !on_altstack;

    // Top of the alternate stack if requested, otherwise below the 128-byte red zone
    // of the interrupted stack (System V AMD64 ABI).
    let base = if use_altstack {
        altstack.sp + altstack.size
    } else {
        context.rsp as usize - 128
    };

    let align = |x: usize| x & !0xF;

    let info_addr = align(base - size_of::<siginfo_t>());
    let uc_addr = align(info_addr - size_of::<Ucontext>());
    let sf_addr = align(uc_addr - size_of::<SignalFrame>());
    let ret_addr = sf_addr - 8;

    let mut gregs = [0u64; 16];
    gregs[0] = context.rax;
    gregs[1] = context.rbx;
    gregs[2] = context.rcx;
    gregs[3] = context.rdx;
    gregs[4] = context.rsi;
    gregs[5] = context.rdi;
    gregs[6] = context.rbp;
    gregs[7] = context.rsp;
    gregs[8] = context.r8;
    gregs[9] = context.r9;
    gregs[10] = context.r10;
    gregs[11] = context.r11;
    gregs[12] = context.r12;
    gregs[13] = context.r13;
    gregs[14] = context.r14;
    gregs[15] = context.r15;

    let ucontext = Ucontext {
        uc_link: 0,
        uc_stack: UserStack {
            ss_sp: altstack.sp as u64,
            ss_size: altstack.size as u64,
            ss_flags: altstack.flags,
            _pad: 0,
        },
        uc_mcontext: Mcontext {
            oldmask: delivery.old_mask.as_raw(),
            gregs,
            pc: context.rip,
            ..Default::default()
        },
        uc_sigmask: delivery.old_mask.as_raw(),
    };

    let frame = SignalFrame {
        saved_mask: delivery.old_mask.as_raw(),
        saved_context: *context,
    };

    let ok = UserPtr::<siginfo_t>::new(VirtAddr::new(info_addr))
        .write(delivery.info)
        .is_some()
        && UserPtr::<Ucontext>::new(VirtAddr::new(uc_addr))
            .write(ucontext)
            .is_some()
        && UserPtr::<SignalFrame>::new(VirtAddr::new(sf_addr))
            .write(frame)
            .is_some()
        && UserPtr::<u64>::new(VirtAddr::new(ret_addr))
            .write(delivery.restorer as u64)
            .is_some();
    if !ok {
        Process::exit(State::Signaled(Signal::SigSegv));
    }

    // Hand control to the handler.
    context.rip = delivery.handler as u64;
    context.rdi = delivery.signal as u64;
    context.rsi = info_addr as u64;
    context.rdx = uc_addr as u64;
    context.rsp = ret_addr as u64;
    context.rflags &= !consts::RFLAGS_DF;
}

pub(in crate::arch) fn restore_signal_frame(context: &mut Context) {
    let ptr = UserPtr::<SignalFrame>::new(VirtAddr::new(context.rsp as usize));
    let Some(frame) = ptr.read() else {
        Process::exit(State::Signaled(Signal::SigSegv));
    };

    // Restore the saved context.
    *context = frame.saved_context;

    // Make sure the user doesn't do anything funky with the context.
    context.cs = offset_of!(Gdt, user_code64) as u64 | consts::CPL_USER as u64;
    context.ss = offset_of!(Gdt, user_data) as u64 | consts::CPL_USER as u64;
    context.rflags = (frame.saved_context.rflags & 0xDD5) | 0x202;

    // Restore the signal mask.
    let task = Scheduler::get_current();
    let mut sig_state = task.signal.lock();
    sig_state.mask = SignalSet::from_raw(frame.saved_mask);
    sig_state.mask.sanitize_mask();
}
