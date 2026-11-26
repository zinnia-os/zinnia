// SPDX-License-Identifier: BSD-3-Clause

#include <dev/acpi/acpi.h>
#include <lib/irq.h>
#include <string.h>
#include <uacpi/namespace.h>
#include <uacpi/resources.h>
#include <uacpi/status.h>

struct resource_find {
    enum acpi_resource_type type;
    size_t index;
    struct acpi_resource* out;
    bool found;
};

static uacpi_namespace_node* node_of(struct acpi_device* dev) {
    return (uacpi_namespace_node*)dev;
}

void acpi_device_name(struct acpi_device* dev, char out[static 5]) {
    const uacpi_object_name name = uacpi_namespace_node_name(node_of(dev));
    memcpy(out, name.text, sizeof(name.text));
    out[4] = 0;
}

static void set_irq(struct acpi_resource* out, uint32_t irq, uint8_t triggering, uint8_t polarity, bool isa) {
    out->type = ACPI_RESOURCE_IRQ;
    out->irq.gsi = irq;
    out->irq.trigger = triggering == UACPI_TRIGGERING_LEVEL ? IRQ_TRIGGER_LEVEL : IRQ_TRIGGER_EDGE;
    out->irq.polarity = polarity == UACPI_POLARITY_ACTIVE_LOW ? IRQ_POLARITY_LOW : IRQ_POLARITY_HIGH;
    if (isa)
        irq_isa_override(irq, &out->irq.gsi, &out->irq.trigger, &out->irq.polarity);
}

static void set_io(struct acpi_resource* out, uint64_t base, uint64_t length) {
    out->type = ACPI_RESOURCE_IO;
    out->io.base = base;
    out->io.length = length;
}

static void set_mem(struct acpi_resource* out, uint64_t base, uint64_t length) {
    out->type = ACPI_RESOURCE_MEM;
    out->mem.base = base;
    out->mem.length = length;
}

static bool set_address(
    struct acpi_resource* out,
    const uacpi_resource_address_common* common,
    uint64_t base,
    uint64_t length
) {
    switch (common->type) {
    case UACPI_RANGE_MEMORY:
        set_mem(out, base, length);
        return true;
    case UACPI_RANGE_IO:
        set_io(out, base, length);
        return true;
    default:
        return false;
    }
}

static bool convert(const uacpi_resource* r, struct acpi_resource* out) {
    switch (r->type) {
    case UACPI_RESOURCE_TYPE_IO:
        set_io(out, r->io.minimum, r->io.length);
        return true;
    case UACPI_RESOURCE_TYPE_FIXED_IO:
        set_io(out, r->fixed_io.address, r->fixed_io.length);
        return true;
    case UACPI_RESOURCE_TYPE_MEMORY32:
        set_mem(out, r->memory32.minimum, r->memory32.length);
        return true;
    case UACPI_RESOURCE_TYPE_FIXED_MEMORY32:
        set_mem(out, r->fixed_memory32.address, r->fixed_memory32.length);
        return true;
    case UACPI_RESOURCE_TYPE_ADDRESS16:
        return set_address(out, &r->address16.common, r->address16.minimum, r->address16.address_length);
    case UACPI_RESOURCE_TYPE_ADDRESS32:
        return set_address(out, &r->address32.common, r->address32.minimum, r->address32.address_length);
    case UACPI_RESOURCE_TYPE_ADDRESS64:
        return set_address(out, &r->address64.common, r->address64.minimum, r->address64.address_length);
    case UACPI_RESOURCE_TYPE_IRQ:
        if (!r->irq.num_irqs)
            return false;
        set_irq(out, r->irq.irqs[0], r->irq.triggering, r->irq.polarity, true);
        return true;
    case UACPI_RESOURCE_TYPE_EXTENDED_IRQ:
        if (!r->extended_irq.num_irqs)
            return false;
        set_irq(out, r->extended_irq.irqs[0], r->extended_irq.triggering, r->extended_irq.polarity, false);
        return true;
    default:
        return false;
    }
}

static uacpi_iteration_decision find_resource(void* user, uacpi_resource* r) {
    struct resource_find* find = user;
    struct acpi_resource res;
    if (!convert(r, &res) || res.type != find->type)
        return UACPI_ITERATION_DECISION_CONTINUE;
    if (find->index--)
        return UACPI_ITERATION_DECISION_CONTINUE;

    *find->out = res;
    find->found = true;
    return UACPI_ITERATION_DECISION_BREAK;
}

errno_t acpi_device_resource(
    struct acpi_device* dev,
    enum acpi_resource_type type,
    size_t index,
    struct acpi_resource* out
) {
    struct resource_find find = {.type = type, .index = index, .out = out};
    uacpi_status ret = uacpi_for_each_device_resource(node_of(dev), "_CRS", find_resource, &find);
    if (ret == UACPI_STATUS_NOT_FOUND)
        return ENOENT;
    if (ret != UACPI_STATUS_OK)
        return EIO;
    return find.found ? 0 : ENOENT;
}
