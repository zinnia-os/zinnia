use crate::{
    clock::{ClockError, ClockSource},
    memory::{UnsafeMemoryView, VmCacheType, view::MmioView},
};
use alloc::boxed::Box;
use uacpi_sys::{
    UACPI_STATUS_OK, acpi_hpet, uacpi_table, uacpi_table_find_by_signature, uacpi_table_unref,
};

pub struct Hpet {
    regs: MmioView,
    period: u32,
}

mod regs {
    use crate::memory::view::Register;

    pub const CAPABILITIES: Register<u64> = Register::new(0);
    pub const CONFIGURATION: Register<u64> = Register::new(0x10);
    pub const MAIN_COUNTER: Register<u64> = Register::new(0xF0);
}

impl ClockSource for Hpet {
    fn name(&self) -> &'static str {
        "hpet"
    }

    fn get_priority(&self) -> u8 {
        75
    }

    fn reset(&mut self) {
        unsafe { self.regs.write_reg(regs::MAIN_COUNTER, 0_u64).unwrap() };
    }

    fn get_elapsed_ns(&self) -> usize {
        let counter = unsafe { self.regs.read_reg(regs::MAIN_COUNTER).unwrap().value() };
        return (counter * self.period as u64 / 1_000_000) as usize;
    }
}

impl Hpet {
    pub fn new() -> Result<Self, ClockError> {
        let mut table = uacpi_table::default();
        let uacpi_status =
            unsafe { uacpi_table_find_by_signature(c"HPET".as_ptr(), &raw mut table) };

        if uacpi_status != UACPI_STATUS_OK {
            return Err(ClockError::Unavailable);
        }

        let hpet = unsafe { table.__bindgen_anon_1.ptr } as *const acpi_hpet;
        let mmio = unsafe {
            MmioView::new(
                ((*hpet).address.address as usize).into(),
                0x1000,
                VmCacheType::Uncacheable,
            )
        };
        unsafe { uacpi_table_unref(&raw mut table) };

        // Enable timer.
        unsafe {
            mmio.write_reg(
                regs::CONFIGURATION,
                mmio.read_reg(regs::CONFIGURATION).unwrap().value() | 1,
            )
            .unwrap()
        };

        let caps = unsafe { mmio.read_reg(regs::CAPABILITIES).unwrap().value() };
        let period = (caps >> 32) as u32;

        return Ok(Hpet { regs: mmio, period });
    }
}

#[initgraph::task(
    name = "arch.x86_64.hpet",
    depends = [crate::device::acpi::TABLES_STAGE],
)]
pub fn HPET_STAGE() {
    if let Ok(x) = Hpet::new() {
        _ = crate::clock::switch(Box::new(x));
    }
}
