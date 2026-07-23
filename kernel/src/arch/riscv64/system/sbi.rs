use crate::log::{self, LoggerSink};
use alloc::boxed::Box;

const DBCN: u64 = 0x4442434E;

pub fn call(eid: u64, fid: u64, a0: u64) -> (u64, u64) {
    unsafe {
        let mut result0;
        let mut result1;
        core::arch::asm!(
            "ecall",
            in("a7") eid,
            in("a6") fid,
            in("a0") a0,
            lateout("a0") result0,
            lateout("a1") result1,
        );
        (result0, result1)
    }
}

pub struct SbiLogger;

impl LoggerSink for SbiLogger {
    fn name(&self) -> &'static str {
        "sbi"
    }

    fn write(&mut self, input: &[u8]) {
        for ch in input {
            call(DBCN, 2, *ch as _);
        }
    }
}

#[task(
    name = "arch.riscv64.sbi-serial",
    entails = [crate::arch::EARLY_INIT_STAGE],
)]
fn SBI_SERIAL_STAGE() {
    log::add_sink(Box::new(SbiLogger));
}
