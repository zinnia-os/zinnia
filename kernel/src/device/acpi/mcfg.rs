use crate::{
    device::pci::{self, Access, EcamPciAccess},
    memory::{
        pmm::KernelAlloc,
        virt::{VmCacheType, VmFlags, mmu::PageTable},
    },
};
use alloc::{boxed::Box, vec::Vec};
use uacpi_sys::{
    UACPI_STATUS_OK, acpi_mcfg, acpi_mcfg_allocation, acpi_sdt_hdr, uacpi_table,
    uacpi_table_find_by_signature, uacpi_table_unref,
};

#[initgraph::task(
    name = "device.acpi.mcfg",
    depends = [
        super::TABLES_STAGE,
        crate::memory::MEMORY_STAGE
    ],
    entails = [super::INIT_STAGE]
)]
pub fn MCFG_STAGE() {
    unsafe {
        let mut table = uacpi_table::default();
        let status = uacpi_table_find_by_signature(c"MCFG".as_ptr(), &raw mut table);
        if status != UACPI_STATUS_OK {
            return;
        }

        let mcfg_ptr = table.__bindgen_anon_1.ptr as *const uacpi_sys::acpi_mcfg;
        let mcfg = mcfg_ptr.read_unaligned();

        let entry_count = (mcfg.hdr.length as usize - size_of::<acpi_sdt_hdr>())
            / size_of::<acpi_mcfg_allocation>();

        let mut accesses = Vec::new();
        for i in 0..entry_count {
            let entry = mcfg_ptr
                .byte_add(size_of::<acpi_mcfg>())
                .cast::<acpi_mcfg_allocation>()
                .add(i)
                .read_unaligned();

            let addr = PageTable::get_kernel()
                .map_memory::<KernelAlloc>(
                    (entry.address).into(),
                    VmFlags::Read | VmFlags::Write,
                    VmCacheType::Normal,
                    ((entry.end_bus - entry.start_bus) as usize + 1) << 20, // + 1 because the range is inclusive
                )
                .unwrap();

            accesses.push(Box::new(EcamPciAccess {
                base: addr as _,
                segment: entry.segment,
                start_bus: entry.start_bus,
                end_bus: entry.end_bus,
            }) as Box<dyn Access>);
        }
        uacpi_table_unref(&mut table);

        // We have an MCFG, so reinitialize the PciAccess callbacks.
        pci::ACCESS.init(accesses);
    };
}
