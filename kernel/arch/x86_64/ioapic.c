// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ioapic: " fmt

#include <arch/irq.h>
#include <arch/mem.h>
#include <arch/x86_64/apic.h>
#include <arch/x86_64/percpu.h>
#include <dev/acpi/acpi.h>
#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/mmio.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/spin.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <uacpi/acpi.h>
#include <uacpi/status.h>
#include <uacpi/tables.h>

constexpr size_t ISA_IRQS = 16;

constexpr mmio_reg32 IOAPIC_REG_SELECT = {.offset = 0x00};
constexpr mmio_reg32 IOAPIC_REG_WINDOW = {.offset = 0x10};

constexpr uint32_t IOAPIC_VERSION = 0x01;
constexpr uint32_t IOAPIC_REDIRECT = 0x10;

constexpr uint64_t IOAPIC_RTE_POLARITY_LOW = 1 << 13;
constexpr uint64_t IOAPIC_RTE_TRIGGER_LEVEL = 1 << 15;
constexpr uint64_t IOAPIC_RTE_MASKED = 1 << 16;

struct ioapic_line {
    struct irq_line line;
    struct ioapic* ioapic;
    uint32_t pin;
    uint32_t lapic;
    uint8_t vector;
};

struct ioapic {
    void* regs;
    struct ioapic_line** lines;
    struct spinlock lock;
    uint32_t gsi_base;
    uint32_t num_pins;
};

struct isa_override {
    uint32_t gsi;
    uint16_t flags;
    bool present;
};

static struct ioapic* ioapics;
static size_t num_ioapics;
static struct isa_override isa_overrides[ISA_IRQS];
static struct spinlock lines_lock;

static uint32_t ioapic_read(struct ioapic* ioapic, uint32_t reg) {
    mmio_reg_write(ioapic->regs, IOAPIC_REG_SELECT, reg);
    return mmio_reg_read(ioapic->regs, IOAPIC_REG_WINDOW);
}

static void ioapic_write(struct ioapic* ioapic, uint32_t reg, uint32_t value) {
    mmio_reg_write(ioapic->regs, IOAPIC_REG_SELECT, reg);
    mmio_reg_write(ioapic->regs, IOAPIC_REG_WINDOW, value);
}

static void write_rte(struct ioapic* ioapic, uint32_t pin, uint64_t rte) {
    ioapic_write(ioapic, IOAPIC_REDIRECT + pin * 2, IOAPIC_RTE_MASKED);
    ioapic_write(ioapic, IOAPIC_REDIRECT + pin * 2 + 1, rte >> 32);
    ioapic_write(ioapic, IOAPIC_REDIRECT + pin * 2, rte);
}

static void update_mask(struct ioapic_line* line, bool masked) {
    struct ioapic* ioapic = line->ioapic;
    spin_lock(&ioapic->lock);
    uint32_t low = ioapic_read(ioapic, IOAPIC_REDIRECT + line->pin * 2);
    low = masked ? low | IOAPIC_RTE_MASKED : low & ~IOAPIC_RTE_MASKED;
    ioapic_write(ioapic, IOAPIC_REDIRECT + line->pin * 2, low);
    spin_unlock(&ioapic->lock);
}

static enum irq_mode ioapic_set_config(
    struct irq_line* self,
    enum irq_trigger_mode trigger,
    enum irq_polarity polarity
) {
    struct ioapic_line* line = containerof(self, struct ioapic_line, line);
    uint64_t rte = line->vector | ((uint64_t)line->lapic << 56);
    if (polarity == IRQ_POLARITY_LOW)
        rte |= IOAPIC_RTE_POLARITY_LOW;
    if (trigger == IRQ_TRIGGER_LEVEL)
        rte |= IOAPIC_RTE_TRIGGER_LEVEL;

    spin_lock(&line->ioapic->lock);
    write_rte(line->ioapic, line->pin, rte);
    spin_unlock(&line->ioapic->lock);
    return IRQ_MODE_EOI | IRQ_MODE_MASKABLE;
}

static void ioapic_mask(struct irq_line* self) {
    update_mask(containerof(self, struct ioapic_line, line), true);
}

static void ioapic_unmask(struct irq_line* self) {
    update_mask(containerof(self, struct ioapic_line, line), false);
}

static void ioapic_eoi(struct irq_line* self) {
    lapic_eoi(&percpu_ref(arch_data)->lapic);
}

errno_t arch_irq_get_gsi(uint32_t gsi, struct irq_line** out) {
    // Find the IOAPIC that handles this GSI.
    struct ioapic* ioapic = nullptr;
    for (size_t i = 0; i < num_ioapics; i++) {
        if (gsi >= ioapics[i].gsi_base && gsi < ioapics[i].gsi_base + ioapics[i].num_pins) {
            ioapic = &ioapics[i];
            break;
        }
    }
    if (!ioapic)
        return ENOENT;
    const uint32_t pin = gsi - ioapic->gsi_base;

    spin_lock(&lines_lock);
    struct ioapic_line* line = ioapic->lines[pin];
    if (line) {
        spin_unlock(&lines_lock);
        *out = &line->line;
        return 0;
    }
    spin_unlock(&lines_lock);

    line = mem_alloc(sizeof(*line), 0);
    if (!line)
        return ENOMEM;
    line->line.set_config = ioapic_set_config;
    line->line.mask = ioapic_mask;
    line->line.unmask = ioapic_unmask;
    line->line.eoi = ioapic_eoi;
    line->ioapic = ioapic;
    line->pin = pin;

    errno_t status = apic_alloc_vector(&line->line, &line->vector, &line->lapic);
    if (status) {
        mem_free(line, sizeof(*line));
        return status;
    }

    spin_lock(&lines_lock);
    if (ioapic->lines[pin]) {
        spin_unlock(&lines_lock);
        apic_free_vector(line->vector);
        mem_free(line, sizeof(*line));
        return arch_irq_get_gsi(gsi, out);
    }
    ioapic->lines[pin] = line;
    spin_unlock(&lines_lock);

    *out = &line->line;
    return 0;
}

bool arch_irq_isa_override(
    uint8_t isa_irq,
    uint32_t* out_gsi,
    enum irq_trigger_mode* out_trigger,
    enum irq_polarity* out_polarity
) {
    if (isa_irq >= ISA_IRQS || !isa_overrides[isa_irq].present)
        return false;

    const struct isa_override* iso = &isa_overrides[isa_irq];
    *out_gsi = iso->gsi;
    switch (iso->flags & ACPI_MADT_POLARITY_MASK) {
    case ACPI_MADT_POLARITY_ACTIVE_HIGH:
        *out_polarity = IRQ_POLARITY_HIGH;
        break;
    case ACPI_MADT_POLARITY_ACTIVE_LOW:
        *out_polarity = IRQ_POLARITY_LOW;
        break;
    }
    switch (iso->flags & ACPI_MADT_TRIGGERING_MASK) {
    case ACPI_MADT_TRIGGERING_EDGE:
        *out_trigger = IRQ_TRIGGER_EDGE;
        break;
    case ACPI_MADT_TRIGGERING_LEVEL:
        *out_trigger = IRQ_TRIGGER_LEVEL;
        break;
    }
    return true;
}

static uacpi_iteration_decision count_ioapics(uacpi_handle user, struct acpi_entry_hdr* hdr) {
    size_t* count = user;
    if (hdr->type == ACPI_MADT_ENTRY_TYPE_IOAPIC)
        (*count)++;
    return UACPI_ITERATION_DECISION_CONTINUE;
}

static uacpi_iteration_decision parse_madt_entry(uacpi_handle, struct acpi_entry_hdr* hdr) {
    switch (hdr->type) {
    case ACPI_MADT_ENTRY_TYPE_IOAPIC: {
        // Map the IOAPIC and mask all of its pins.
        const struct acpi_madt_ioapic* entry = (const struct acpi_madt_ioapic*)hdr;
        uintptr_t vaddr = vm_alloc(arch_mem_page_size());
        if (!vaddr)
            return UACPI_ITERATION_DECISION_CONTINUE;
        pmap_map(&kernel_vma.pmap, vaddr, entry->address, PTE_READ | PTE_WRITE, CACHE_MMIO);

        struct ioapic* ioapic = &ioapics[num_ioapics];
        ioapic->regs = (void*)vaddr;
        ioapic->gsi_base = entry->gsi_base;
        ioapic->num_pins = ((ioapic_read(ioapic, IOAPIC_VERSION) >> 16) & 0xFF) + 1;
        spin_init(&ioapic->lock);
        ioapic->lines = mem_alloc(ioapic->num_pins * sizeof(*ioapic->lines), 0);
        if (!ioapic->lines) {
            pmap_unmap(&kernel_vma.pmap, vaddr);
            vm_free(vaddr, arch_mem_page_size());
            return UACPI_ITERATION_DECISION_CONTINUE;
        }

        for (uint32_t pin = 0; pin < ioapic->num_pins; pin++)
            write_rte(ioapic, pin, IOAPIC_RTE_MASKED);

        num_ioapics++;
        log("IOAPIC %u at %#x handles GSIs %u-%u\n",
            entry->id,
            entry->address,
            ioapic->gsi_base,
            ioapic->gsi_base + ioapic->num_pins - 1);
        break;
    }
    case ACPI_MADT_ENTRY_TYPE_INTERRUPT_SOURCE_OVERRIDE:
        auto iso = (const struct acpi_madt_interrupt_source_override*)hdr;
        if (iso->bus == 0 && iso->source < ISA_IRQS) {
            isa_overrides[iso->source] = (struct isa_override){
                .gsi = iso->gsi,
                .flags = iso->flags,
                .present = true,
            };
        }
        break;
    }
    return UACPI_ITERATION_DECISION_CONTINUE;
}

static void ioapic_init(struct boot_info*) {
    uacpi_table table;
    if (uacpi_table_find_by_signature(ACPI_MADT_SIGNATURE, &table) != UACPI_STATUS_OK) {
        log_warn("No MADT found\n");
        return;
    }

    size_t capacity = 0;
    uacpi_status status = uacpi_for_each_subtable(table.hdr, sizeof(struct acpi_madt), count_ioapics, &capacity);
    if (status != UACPI_STATUS_OK)
        log_warn("Failed to parse the MADT: %s\n", uacpi_status_to_string(status));

    if (capacity) {
        ioapics = mem_alloc(capacity * sizeof(*ioapics), 0);
        if (!ioapics) {
            log_err("Unable to allocate %zu controllers\n", capacity);
            uacpi_table_unref(&table);
            return;
        }
    }

    uacpi_for_each_subtable(table.hdr, sizeof(struct acpi_madt), parse_madt_entry, nullptr);
    uacpi_table_unref(&table);
}
INIT_TASK(ioapic_stage, "arch.x86_64.ioapic", ioapic_init);
INIT_DEPENDS(ioapic_stage, acpi_stage, percpu_stage);
INIT_ENTAILS(ioapic_stage, acpi_namespace_stage);
