use crate::{
    irq::lock::IrqGuard,
    memory::{VirtAddr, virt::KERNEL_STACK_SIZE},
    posix::errno::{EResult, Errno},
    process::{signal::SignalSet, task::Task},
};
use core::{arch::naked_asm, mem::offset_of};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Context {
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub tp: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub s0: u64,
    pub s1: u64,
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
}

impl Context {
    pub fn syscall_number(&self) -> usize {
        self.a7 as usize
    }

    pub fn arg0(&self) -> usize {
        self.a0 as usize
    }

    pub fn arg1(&self) -> usize {
        self.a1 as usize
    }

    pub fn arg2(&self) -> usize {
        self.a2 as usize
    }

    pub fn arg3(&self) -> usize {
        self.a3 as usize
    }

    pub fn arg4(&self) -> usize {
        self.a4 as usize
    }

    pub fn arg5(&self) -> usize {
        self.a5 as usize
    }

    pub fn set_return(&mut self, val: usize, err: usize) {
        self.a0 = val as _;
        self.a1 = err as _;
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct TaskContext {
    pub sp: u64,
    pub tp: u64,
}

#[repr(C)]
struct TaskFrame {
    pub ra: u64,
    pub gp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
}

pub unsafe fn preempt_disable() {
    // TODO
}

pub unsafe fn preempt_enable() -> bool {
    // TODO
    true
}

pub unsafe fn switch(from: *const Task, to: *const Task, irq_guard: IrqGuard) {
    unsafe {
        let from = from.as_ref().unwrap();
        let to = to.as_ref().unwrap();

        let mut from_context = from.task_context.lock();
        let to_context = to.task_context.lock();

        let old_sp = &raw mut from_context.sp;
        let new_sp = to_context.sp;

        drop(irq_guard);
        perform_switch(old_sp, new_sp);
    }
}

#[unsafe(naked)]
pub unsafe extern "C" fn perform_switch(old_sp: *mut u64, new_sp: u64) {
    naked_asm!(
        "addi sp, sp, -{size}", // Make room for all regs.
        "sd ra, {ra}(sp)",
        "sd gp, {gp}(sp)",
        "sd s0, {s0}(sp)",
        "sd s1, {s1}(sp)",
        "sd s2, {s2}(sp)",
        "sd s3, {s3}(sp)",
        "sd s4, {s4}(sp)",
        "sd s5, {s5}(sp)",
        "sd s6, {s6}(sp)",
        "sd s7, {s7}(sp)",
        "sd s8, {s8}(sp)",
        "sd s9, {s9}(sp)",
        "sd s10, {s10}(sp)",
        "sd s11, {s11}(sp)",
        "sd sp, 0(a0)", // a0 = old_sp
        "mv sp, a1",    // a1 = new_sp
        "ld ra, {ra}(sp)",
        "ld gp, {gp}(sp)",
        "ld s0, {s0}(sp)",
        "ld s1, {s1}(sp)",
        "ld s2, {s2}(sp)",
        "ld s3, {s3}(sp)",
        "ld s4, {s4}(sp)",
        "ld s5, {s5}(sp)",
        "ld s6, {s6}(sp)",
        "ld s7, {s7}(sp)",
        "ld s8, {s8}(sp)",
        "ld s9, {s9}(sp)",
        "ld s10, {s10}(sp)",
        "ld s11, {s11}(sp)",
        "addi sp, sp, {size}",
        "ret",
        size = const size_of::<TaskFrame>(),
        ra = const offset_of!(TaskFrame, ra),
        gp = const offset_of!(TaskFrame, gp),
        s0 = const offset_of!(TaskFrame, s0),
        s1 = const offset_of!(TaskFrame, s1),
        s2 = const offset_of!(TaskFrame, s2),
        s3 = const offset_of!(TaskFrame, s3),
        s4 = const offset_of!(TaskFrame, s4),
        s5 = const offset_of!(TaskFrame, s5),
        s6 = const offset_of!(TaskFrame, s6),
        s7 = const offset_of!(TaskFrame, s7),
        s8 = const offset_of!(TaskFrame, s8),
        s9 = const offset_of!(TaskFrame, s9),
        s10 = const offset_of!(TaskFrame, s10),
        s11 = const offset_of!(TaskFrame, s11)
    );
}

pub unsafe fn remote_reschedule(cpu: u32) {
    let _ = cpu;
    todo!()
}

pub fn init_task(
    task: &mut TaskContext,
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
    stack_start: VirtAddr,
    is_user: bool,
) -> EResult<()> {
    // Prepare a dummy stack with an entry point function to return to.
    unsafe {
        let frame = ((stack_start.value() + KERNEL_STACK_SIZE) as *mut TaskFrame).sub(1);
        (*frame).s0 = entry as u64;
        (*frame).s1 = arg1 as u64;
        (*frame).s2 = arg2 as u64;
        (*frame).ra = task_entry_thunk as *const () as u64;
        task.sp = frame as u64;

        if is_user {}
    }

    Ok(())
}

#[unsafe(naked)]
unsafe extern "C" fn task_entry_thunk() -> ! {
    naked_asm!(
        "mv a0, s0",
        "mv a1, s1",
        "mv a2, s2",
        "mv ra, zero",
        "la t0, {task_thunk}",
        "jr t0",
        task_thunk = sym crate::sched::task_entry,
    );
}

pub unsafe fn jump_to_user(ip: VirtAddr, sp: VirtAddr) {
    let _ = (ip, sp);
    todo!()
}

pub fn setup_signal_frame(
    _context: &mut Context,
    _handler: usize,
    _signal: u32,
    _mask: SignalSet,
    _restorer: usize,
) {
    todo!("riscv64 signal frame setup not yet implemented")
}

pub fn restore_signal_frame(_context: &mut Context) -> EResult<()> {
    Err(Errno::ENOSYS)
}

pub unsafe fn jump_to_context(context: *mut Context) {
    let _ = context;
    todo!()
}
