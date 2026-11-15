// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "acpi: " fmt

#include <dev/acpi/acpi.h>
#include <dev/pci/pci.h>
#include <lib/print.h>
#include <uacpi/acpi.h>
#include <uacpi/status.h>
#include <uacpi/tables.h>

[[__init]]
static void acpi_mcfg_stage_init(struct boot_info*) {
    uacpi_table table = {};
    if (uacpi_table_find_by_signature(ACPI_MCFG_SIGNATURE, &table) != UACPI_STATUS_OK)
        return;

    struct acpi_mcfg* mcfg = table.ptr;
    size_t count = 0;
    if (mcfg->hdr.length > sizeof(*mcfg))
        count = (mcfg->hdr.length - sizeof(*mcfg)) / sizeof(struct acpi_mcfg_allocation);

    for (size_t i = 0; i < count; i++) {
        struct acpi_mcfg_allocation* entry = &mcfg->entries[i];
        errno_t status = pci_ecam_register(entry->address, entry->segment, entry->start_bus, entry->end_bus);
        if (status != 0)
            log_warn("Failed to register ECAM for segment %hu: %i\n", entry->segment, status);
    }

    uacpi_table_unref(&table);
}
INIT_TASK(acpi_mcfg_stage, "dev.acpi.mcfg", acpi_mcfg_stage_init);
INIT_DEPENDS(acpi_mcfg_stage, acpi_stage);
INIT_DEPENDS(acpi_mcfg_stage, mem_stage);
INIT_ENTAILS(acpi_mcfg_stage, pci_stage);
