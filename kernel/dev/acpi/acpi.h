// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/init.h>
#include <lib/initgraph.h>
#include <lib/types.h>
#include <lib/workqueue.h>
#include <stdint.h>

INIT_EXTERN(acpi_stage);
INIT_EXTERN(acpi_namespace_stage);

extern phys_t acpi_rsdp_addr;
extern struct workqueue* acpi_workqueue;

errno_t acpi_reboot();
errno_t acpi_power_off();

struct acpi_match {
    const char* hid;
    uintptr_t data;
};

#define ACPI_MATCH_HID(id) .hid = (id)
