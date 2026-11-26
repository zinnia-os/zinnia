// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/init.h>
#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/types.h>
#include <lib/workqueue.h>
#include <stddef.h>
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

struct acpi_device;

struct acpi_driver {
    const char* name;
    const struct acpi_match* match;
    const size_t num_matches;
    errno_t (*probe)(struct acpi_device* dev, const struct acpi_match* match);
};

enum acpi_resource_type : uint8_t {
    ACPI_RESOURCE_IO,
    ACPI_RESOURCE_MEM,
    ACPI_RESOURCE_IRQ,
};

struct acpi_resource {
    enum acpi_resource_type type;
    union {
        struct {
            uint16_t base;
            uint16_t length;
        } io;
        struct {
            phys_t base;
            size_t length;
        } mem;
        struct {
            uint32_t gsi;
            enum irq_trigger_mode trigger;
            enum irq_polarity polarity;
        } irq;
    };
};

void acpi_device_name(struct acpi_device* dev, char out[static 5]);
errno_t acpi_device_resource(
    struct acpi_device* dev,
    enum acpi_resource_type type,
    size_t index,
    struct acpi_resource* out
);

bool acpi_namespace_ready();
errno_t acpi_driver_register(struct acpi_driver* driver);
