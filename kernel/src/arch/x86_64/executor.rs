use super::{sched, sched::Context};
use crate::{memory::stack::KernelStack, posix::errno::EResult, process::task::Task};
use alloc::boxed::Box;
use core::{arch::asm, mem::offset_of};

#[repr(C)]
#[derive(Default, Debug, Clone)]
pub struct Executor {
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

#[repr(C)]
pub struct ForkedImage {
    rbx: usize,
    rbp: usize,
    r12: usize,
    r13: usize,
    r14: usize,
    r15: usize,
    rsp: usize,
    rip: usize,
    rflags: usize,
}

impl ForkedImage {
    pub const fn ip(&self) -> usize {
        self.rip
    }

    pub const fn sp(&self) -> usize {
        self.rsp
    }

    pub const fn flags(&self) -> usize {
        self.rflags
    }
}

pub struct FrameImage<'a> {
    frame: &'a mut Context,
}

impl<'a> FrameImage<'a> {
    pub fn new(frame: &'a mut Context) -> Self {
        Self { frame }
    }

    pub const fn frame(&self) -> &Context {
        self.frame
    }

    pub fn frame_mut(&mut self) -> &mut Context {
        self.frame
    }

    pub const fn ip(&self) -> usize {
        self.frame.rip as usize
    }

    pub const fn sp(&self) -> usize {
        self.frame.rsp as usize
    }

    pub const fn flags(&self) -> usize {
        self.frame.rflags as usize
    }
}

pub unsafe fn switch(from: *const Task, to: *const Task) -> *mut Task {
    unsafe { sched::switch(from, to) }
}

pub fn run_on_stack_raw(
    stack_top: usize,
    entry: extern "C" fn(usize, usize) -> !,
    arg: usize,
) -> ! {
    sched::run_on_stack_raw(stack_top, entry, arg)
}

pub fn fork_current(entry: extern "C" fn(*const ForkedImage, usize) -> !, arg: usize) {
    let mut image = ForkedImage {
        rbx: 0,
        rbp: 0,
        r12: 0,
        r13: 0,
        r14: 0,
        r15: 0,
        rsp: 0,
        rip: 0,
        rflags: 0,
    };

    unsafe {
        asm!(
            "mov [{image} + {rbx}], rbx",
            "mov [{image} + {rbp}], rbp",
            "mov [{image} + {r12}], r12",
            "mov [{image} + {r13}], r13",
            "mov [{image} + {r14}], r14",
            "mov [{image} + {r15}], r15",
            "mov [{image} + {rsp}], rsp",
            "lea rax, [rip + 2f]",
            "mov [{image} + {rip}], rax",
            "pushfq",
            "pop [{image} + {rflags}]",
            "mov rdi, {image}",
            "mov rsi, {arg}",
            "call {entry}",
            "ud2",
            "2:",
            image = in(reg) &mut image,
            arg = in(reg) arg,
            entry = in(reg) entry,
            rbx = const offset_of!(ForkedImage, rbx),
            rbp = const offset_of!(ForkedImage, rbp),
            r12 = const offset_of!(ForkedImage, r12),
            r13 = const offset_of!(ForkedImage, r13),
            r14 = const offset_of!(ForkedImage, r14),
            r15 = const offset_of!(ForkedImage, r15),
            rsp = const offset_of!(ForkedImage, rsp),
            rip = const offset_of!(ForkedImage, rip),
            rflags = const offset_of!(ForkedImage, rflags),
            out("rax") _,
            out("rdi") _,
            out("rsi") _,
            out("rdx") _,
            out("rcx") _,
            out("r8") _,
            out("r9") _,
            out("r10") _,
            out("r11") _,
        );
    }
}
