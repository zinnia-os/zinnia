use crate::{
    arch::irq::wait_for_irq,
    {
        percpu::CpuData,
        posix::errno::{EResult, Errno},
    },
};
use core::arch::naked_asm;

// TODO
fn stvec() {
    panic!("exception!");
}

pub fn setup_bsp() {
    unsafe {
        core::arch::asm!("la tp, {percpu}", percpu = sym crate::percpu::LD_PERCPU_START);
        core::arch::asm!("csrw stvec, {stvec}", stvec = in(reg) stvec);
    }
}

pub fn get_frame_pointer() -> usize {
    unsafe {
        let result;
        core::arch::asm!("mv {result}, fp", result = out(reg) result);
        return result;
    }
}

pub fn get_per_cpu() -> *mut CpuData {
    unsafe {
        let result;
        core::arch::asm!("mv {result}, tp", result = out(reg) result);
        return result;
    }
}

pub fn archctl(cmd: usize, arg: usize) -> EResult<usize> {
    let _ = arg;
    match cmd {
        _ => Err(Errno::EINVAL),
    }
}

pub fn halt() -> ! {
    loop {
        wait_for_irq();
    }
}

pub fn halt_others() {
    loop {
        wait_for_irq();
    }
}

unsafe extern "C" {
    unsafe static LD_STACK_TOP: u8;
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn _start() {
    naked_asm!(
        "la sp, {stack}",
        "la t0, {entry}",
        "jr t0",
        "unimp",
        stack = sym LD_STACK_TOP,
        entry = sym crate::boot::entry,
    );
}
