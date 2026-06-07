use super::internal;
use crate::{
    memory::{VirtAddr, stack::KernelStack},
    posix::errno::EResult,
    process::task::Task,
};
use core::{fmt::Debug, mem::MaybeUninit};

pub use internal::sched::{Context, SyscallRestart};
assert_trait_impl!(TaskContext, Debug);
assert_trait_impl!(Context, Default);
assert_trait_impl!(Context, Clone);
assert_trait_impl!(Context, Copy);

pub use internal::sched::TaskContext;
assert_trait_impl!(TaskContext, Debug);
assert_trait_impl!(TaskContext, Default);
assert_trait_impl!(TaskContext, Clone);

/// Disables preemption.
/// # Safety
/// The implementation of this function must be an atomic operation for this to be memory safe!
#[inline]
pub unsafe fn preempt_disable() {
    unsafe { internal::sched::preempt_disable() };
}

/// Enables preemption. Returns true, if a reschedule was queued.
/// # Safety
/// The implementation of this function must be an atomic operation for this to be memory safe!
#[inline]
pub unsafe fn preempt_enable() -> bool {
    unsafe { internal::sched::preempt_enable() }
}

/// Switches the current CPU context from one task to another.
/// # Safety
/// The caller must ensure that `from` and `to` are both valid tasks and
/// that both arguments do not point to the same task.
pub unsafe fn switch(from: *const Task, to: *const Task) -> *mut Task {
    unsafe { internal::sched::switch(from, to) }
}

/// Performs a reschedule on a given CPU.
/// # Safety
/// The implementation must make sure to take the preemption state into account.
pub unsafe fn remote_reschedule(cpu: u32) {
    unsafe { internal::sched::remote_reschedule(cpu) }
}

/// Sends a TLB shootdown IPI to every online CPU except the current one.
pub fn broadcast_shootdown() {
    internal::sched::broadcast_shootdown()
}

/// Initializes a new task.
pub fn init_task(
    task: &mut TaskContext,
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
    stack: &KernelStack,
    is_user: bool,
) -> EResult<()> {
    internal::sched::init_task(task, entry, arg1, arg2, stack, is_user)
}

/// Executes a function or closure on the provided kernel stack.
pub fn run_on_stack<F: FnMut(usize) -> !>(stack: &KernelStack, f: F) -> ! {
    extern "C" fn run_on_stack_entry<F: FnMut(usize) -> !>(previous_sp: usize, arg: usize) -> ! {
        let f = unsafe { &mut *(arg as *mut F) };

        f(previous_sp)
    }

    let mut top = stack.top().value();
    top -= size_of::<F>().next_multiple_of(align_of::<u128>());

    unsafe {
        let ptr = top as *mut MaybeUninit<F>;
        (*ptr).write(f);
    }

    internal::sched::run_on_stack_raw(stack.top().value(), run_on_stack_entry::<F>, top)
}

/// Transitions to user mode at a specified IP and SP.
/// # Safety
/// `ip` and `sp` have to point to valid and mapped addresses in the current address space.
pub unsafe fn jump_to_user(ip: VirtAddr, sp: VirtAddr) {
    unsafe { internal::sched::jump_to_user(ip, sp) };
}

/// Transitions to a specified context.
/// # Safety
/// `context` has to be allocated on the stack.
pub unsafe fn jump_to_context(context: *mut Context) {
    unsafe { internal::sched::jump_to_context(context) };
}

/// Sets up a signal frame on the user stack, modifying the context to jump to the signal handler.
/// When the handler returns, execution continues via the restorer which calls sigreturn.
pub fn setup_signal_frame(
    context: &mut Context,
    delivery: &crate::process::signal::SignalDelivery,
) {
    internal::sched::setup_signal_frame(context, delivery);
}

/// Restores the original context from a signal frame on the user stack.
/// Called by the sigreturn syscall.
pub fn restore_signal_frame(context: &mut Context) {
    internal::sched::restore_signal_frame(context)
}

// # Note
// This module is only used to ensure the API is correctly implemented,
// since associated functions are more complicated. Not to be used directly.
#[doc(hidden)]
#[allow(unused)]
mod api {
    use super::{Context, SyscallRestart};

    fn set_return(ctx: &mut Context, val: usize, err: usize) {
        ctx.set_return(val, err);
    }

    fn sp(ctx: &Context) -> usize {
        ctx.sp()
    }

    fn return_error(ctx: &Context) -> usize {
        ctx.syscall_error()
    }

    fn snapshot_syscall(ctx: &Context) -> SyscallRestart {
        ctx.snapshot_syscall()
    }

    fn restart_syscall(ctx: &mut Context, restart: &SyscallRestart) {
        ctx.restart_syscall(restart);
    }
}
