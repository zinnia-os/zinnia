use crate::{
    boot::BootInfo,
    irq::IrqLine,
    memory::PhysAddr,
    util::{mutex::spin::SpinMutex, once::Once},
};
use alloc::{boxed::Box, collections::btree_map::BTreeMap};
use core::ffi::c_void;

mod madt;
mod mcfg;
mod uacpi;

static RSDP_ADDRESS: Once<PhysAddr> = Once::new();

pub static GLOBAL_IRQS: SpinMutex<BTreeMap<u32, SpinMutex<Box<dyn IrqLine>>>> =
    SpinMutex::new(BTreeMap::new());

#[initgraph::task(
    name = "system.acpi.tables",
    depends = [crate::memory::MEMORY_STAGE],
)]
pub fn TABLES_STAGE() {
    match BootInfo::get().rsdp_addr {
        Some(rsdp) => unsafe { RSDP_ADDRESS.init(rsdp) },
        None => panic!("No RSDP available, unable to initialize the ACPI subsystem!"),
    };

    // Get an early table window so we can initialize e.g. HPET and MADT.
    let mut early_mem = Box::<[u8]>::new_uninit_slice(4096);

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

#[initgraph::task(
    name = "system.acpi",
    depends = [
        TABLES_STAGE,
        crate::arch::INIT_STAGE,
        crate::clock::CLOCK_STAGE,
        crate::memory::MEMORY_STAGE,
    ],
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
