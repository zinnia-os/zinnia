// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "acpi: " fmt

#include <dev/acpi/acpi.h>
#include <dev/pci/pci.h>
#include <lib/cmdline.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/types.h>
#include <lib/workqueue.h>
#include <mem/alloc.h>
#include <uacpi/event.h>
#include <uacpi/sleep.h>
#include <uacpi/status.h>
#include <uacpi/tables.h>
#include <uacpi/uacpi.h>

phys_t acpi_rsdp_addr = 0;
struct workqueue* acpi_workqueue;

static char table_buf[0x1000];

static bool use_acpi = true;
void use_acpi_option(bool use) {
    use_acpi = use;
}
CMDLINE_OPTION("acpi", "Enable ACPI firmware subsystem", use_acpi_option);

[[__init]]
static bool acpi_stage_init(struct boot_info* info) {
    if (!use_acpi) {
        log_warn("ACPI disabled on the command line!\n");
        return false;
    }

    if (!info->rsdp) {
        log_warn("No RSDP was passed, unable to start ACPI subsystem!\n");
        return false;
    }
    acpi_rsdp_addr = info->rsdp;

    if (uacpi_setup_early_table_access(table_buf, sizeof(table_buf)) != UACPI_STATUS_OK) {
        log_err("Failed to set up early table access!\n");
        return false;
    }

    // If we have both a DTB and RSDP, and ACPI wasn't explicitly disabled or
    // compiled out, we will always prefer the RSDP.
    if (info->dtb) {
        log("Ignoring the device tree in favor of ACPI\n");
        info->dtb = 0;
        info->dtb_size = 0;
    }

    return true;
}
INIT_TASK(acpi_stage, "dev.acpi", acpi_stage_init);
INIT_DEPENDS(acpi_stage, mem_stage);

static bool namespace_done;

[[__init]]
static void acpi_namespace_init(struct boot_info*) {
    errno_t error = workqueue_create("acpi", SCHED_PRIO_WORK_HIGH, &acpi_workqueue);
    if (error) {
        log_err("Failed to create the ACPI workqueue (%i)\n", error);
        return;
    }

    uacpi_status status = uacpi_initialize(0);
    if (status != UACPI_STATUS_OK) {
        log_err("Initialization failed with error \"%s\"!\n", uacpi_status_to_string(status));
        return;
    }

    status = uacpi_namespace_load();
    if (status != UACPI_STATUS_OK) {
        log_err("Namespace loading failed with error \"%s\"!\n", uacpi_status_to_string(status));
        return;
    }

    status = uacpi_namespace_initialize();
    if (status != UACPI_STATUS_OK) {
        log_err("Namespace initialization failed with error \"%s\"!\n", uacpi_status_to_string(status));
        return;
    }

    status = uacpi_finalize_gpe_initialization();
    if (status != UACPI_STATUS_OK)
        log_warn("GPE initialization failed with error \"%s\"!\n", uacpi_status_to_string(status));

    namespace_done = true;
}
INIT_TASK(acpi_namespace_stage, "dev.acpi.namespace", acpi_namespace_init);
INIT_DEPENDS(acpi_namespace_stage, acpi_stage, workqueue_stage);
#if CONFIG_PCI
INIT_DEPENDS(acpi_namespace_stage, pci_stage);
#endif

errno_t acpi_reboot() {
    if (!namespace_done)
        return ENOSYS;

    uacpi_status status = uacpi_reboot();
    log_err("Failed to reset the system: %s\n", uacpi_status_to_string(status));
    return EIO;
}

errno_t acpi_power_off() {
    if (!namespace_done)
        return ENOSYS;

    uacpi_status status = uacpi_prepare_for_sleep_state(UACPI_SLEEP_STATE_S5);
    if (status == UACPI_STATUS_OK) {
        irq_lock();
        status = uacpi_enter_sleep_state(UACPI_SLEEP_STATE_S5);
        irq_unlock();
    }
    log_err("Failed to power off the system: %s\n", uacpi_status_to_string(status));
    return EIO;
}
