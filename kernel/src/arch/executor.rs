use super::internal;
use crate::{memory::stack::KernelStack, posix::errno::EResult, process::task::Task};

pub use internal::executor::{Executor, ForkedImage, FrameImage};

pub fn new_kernel(
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
    stack: &KernelStack,
) -> EResult<Executor> {
    internal::executor::Executor::new_kernel(entry, arg1, arg2, stack)
}

/// Switches the current CPU context from one task executor to another.
///
/// # Safety
/// The caller must ensure that `from` and `to` are valid, distinct live tasks.
pub unsafe fn switch(from: *const Task, to: *const Task) -> *mut Task {
    unsafe { internal::executor::switch(from, to) }
}

pub fn fork_current(entry: extern "C" fn(*const ForkedImage, usize) -> !, arg: usize) {
    internal::executor::fork_current(entry, arg)
}

pub fn run_on_stack_raw(
    stack_top: usize,
    entry: extern "C" fn(usize, usize) -> !,
    arg: usize,
) -> ! {
    internal::executor::run_on_stack_raw(stack_top, entry, arg)
}
