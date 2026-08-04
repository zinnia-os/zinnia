use crate::{boot::BootInfo, memory::PhysAddr, util::once::Once};
use alloc::boxed::Box;
use core::ffi::c_void;

mod mcfg;
mod uacpi;

static RSDP_ADDRESS: Once<PhysAddr> = Once::new();

pub fn power_off() -> uacpi::uacpi_status {
    let status = unsafe { uacpi::uacpi_prepare_for_sleep_state(uacpi::UACPI_SLEEP_STATE_S5) };
    if status != uacpi::UACPI_STATUS_OK {
        return status;
    }

    let irq_state = unsafe { crate::arch::irq::set_irq_state(false) };
    let status = unsafe { uacpi::uacpi_enter_sleep_state(uacpi::UACPI_SLEEP_STATE_S5) };
    unsafe { crate::arch::irq::set_irq_state(irq_state) };
    status
}

pub fn reboot() -> uacpi::uacpi_status {
    unsafe { uacpi::uacpi_reboot() }
}

#[task(
    name = "device.acpi.root",
    depends = [crate::memory::MEMORY_STAGE],
)]
pub fn ACPI_ROOT() -> bool {
    match BootInfo::get().rsdp_addr {
        Some(x) => {
            unsafe { RSDP_ADDRESS.init(x) };
            true
        }
        None => false,
    }
}

#[task(
    name = "device.acpi.tables",
    depends = [ACPI_ROOT],
)]
pub fn TABLES_STAGE() {
    // Get an early table window so we can initialize e.g. HPET and MADT.
    let early_mem = Box::leak(Box::<[u8]>::new_uninit_slice(4096));

    let uacpi_status = unsafe {
        uacpi::uacpi_setup_early_table_access(
            early_mem.as_mut_ptr() as *mut c_void,
            early_mem.len(),
        )
    };

    if uacpi_status != uacpi::UACPI_STATUS_OK {
        error!(
            "acpi: Early table access failed with error {}!\n",
            uacpi_status
        );
        return;
    }
}

#[task(
    name = "system.acpi",
    depends = [
        TABLES_STAGE,
        crate::arch::INIT_STAGE,
        crate::clock::CLOCK_STAGE,
        crate::memory::MEMORY_STAGE,
    ],
    entails = [crate::device::pci::PCI_STAGE],
)]
pub fn INIT_STAGE() {
    let mut uacpi_status = unsafe { uacpi::uacpi_initialize(0) };
    if uacpi_status != uacpi::UACPI_STATUS_OK {
        error!(
            "acpi: Initialization failed with error \"{}\"!",
            uacpi_status
        );
        return;
    }

    uacpi_status = unsafe { uacpi::uacpi_namespace_load() };
    if uacpi_status != uacpi::UACPI_STATUS_OK {
        error!(
            "acpi: Namespace loading failed with error \"{}\"!",
            uacpi_status
        );
        return;
    } else {
        unsafe { uacpi::uacpi_namespace_initialize() };
    }
}
