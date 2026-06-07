use super::sched;
use crate::{memory::stack::KernelStack, posix::errno::EResult, process::task::Task};

pub type Executor = sched::TaskContext;

#[repr(C)]
pub struct ForkedImage;

pub struct FrameImage;

impl Executor {
    pub fn new_kernel(
        entry: extern "C" fn(usize, usize),
        arg1: usize,
        arg2: usize,
        stack: &KernelStack,
    ) -> EResult<Self> {
        let mut this = Self::default();
        sched::init_task(&mut this, entry, arg1, arg2, stack, false)?;
        Ok(this)
    }
}

pub unsafe fn switch(from: *const Task, to: *const Task) -> *mut Task {
    unsafe { sched::switch(from, to) }
}

pub fn fork_current(_entry: extern "C" fn(*const ForkedImage, usize) -> !, _arg: usize) {
    todo!("riscv64 executor forking is not implemented yet")
}

pub fn run_on_stack_raw(
    stack_top: usize,
    entry: extern "C" fn(usize, usize) -> !,
    arg: usize,
) -> ! {
    sched::run_on_stack_raw(stack_top, entry, arg)
}
