use super::internal;
use crate::irq::lock::IrqGuard;
use crate::memory::VirtAddr;
use crate::posix::errno::EResult;
use crate::process::signal::SignalSet;
use crate::process::task::Task;
use core::fmt::Debug;

pub use internal::sched::Context;
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
pub unsafe fn switch(from: *const Task, to: *const Task, irq_guard: IrqGuard) {
    unsafe { internal::sched::switch(from, to, irq_guard) }
}

/// Performs a reschedule on a given CPU.
/// # Safety
/// The implementation must make sure to take the preemption state into account.
pub unsafe fn remote_reschedule(cpu: u32) {
    unsafe { internal::sched::remote_reschedule(cpu) }
}

/// Initializes a new task.
pub fn init_task(
    task: &mut TaskContext,
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
    stack_start: VirtAddr,
    is_user: bool,
) -> EResult<()> {
    internal::sched::init_task(task, entry, arg1, arg2, stack_start, is_user)
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
    handler: usize,
    signal: u32,
    mask: SignalSet,
    restorer: usize,
) {
    internal::sched::setup_signal_frame(context, handler, signal, mask, restorer);
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
    use super::Context;

    fn set_return(ctx: &mut Context, val: usize, err: usize) {
        ctx.set_return(val, err);
    }
}
