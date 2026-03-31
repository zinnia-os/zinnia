use crate::irq::MsiLine;
use core::arch::asm;

pub unsafe fn set_irq_state(value: bool) -> bool {
    let old: u64;
    unsafe {
        asm!(
            "csrr {old}, sie",
            old = out(reg) old,
        );
        if value {
            asm!("csrw sie, 1");
        } else {
            asm!("csrw sie, 0");
        }
    }

    old != 0
}

pub fn get_irq_state() -> bool {
    let old: u64;
    unsafe {
        asm!(
            "csrr {old}, sie",
            old = out(reg) old,
        );
    }

    old != 0
}

pub fn wait_for_irq() {
    unsafe {
        asm!("wfi");
    }
}

pub(in crate::arch) fn allocate_msi() -> Option<Arc<dyn MsiLine>> {
    todo!()
}
