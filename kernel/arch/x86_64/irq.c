// SPDX-License-Identifier: BSD-3-Clause

#include <arch/irq.h>
#include <arch/x86_64/apic.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/percpu.h>
#include <lib/assert.h>
#include <lib/percpu.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <stdcountof.h>
#include <stdint.h>

bool arch_irq_get_state() {
    uint64_t flags;
    asm volatile("pushf; pop %0" : "=r"(flags)::"memory");
    return flags & RFLAGS_IF;
}

bool arch_irq_set_state(bool state) {
    bool old = arch_irq_get_state();
    if (state)
        asm volatile("sti" ::: "memory");
    else
        asm volatile("cli" ::: "memory");
    return old;
}

void arch_irq_wait() {
    asm volatile("hlt");
}

struct apic_msi_line {
    struct msi_line msi;
    uint8_t vector;
    uint32_t lapic;
};

static phys_t apic_msi_msg_addr(struct msi_line* self) {
    struct apic_msi_line* line = containerof(self, struct apic_msi_line, msi);
    return 0xFEE0'0000 | ((phys_t)line->lapic << 12);
}

static uint32_t apic_msi_msg_data(struct msi_line* self) {
    return containerof(self, struct apic_msi_line, msi)->vector;
}

static enum irq_mode apic_msi_set_config(
    struct irq_line* self,
    enum irq_trigger_mode trigger,
    enum irq_polarity polarity
) {
    assert(trigger == IRQ_TRIGGER_EDGE, "MSIs must be edge triggered");
    return IRQ_MODE_EOI;
}

static void apic_msi_mask(struct irq_line* self) {}

static void apic_msi_unmask(struct irq_line* self) {}

static void apic_msi_eoi(struct irq_line* self) {
    lapic_eoi(&percpu_ref(arch_data)->lapic);
}

errno_t apic_alloc_vector(struct irq_line* line, uint8_t* out_vector, uint32_t* out_lapic) {
    irq_lock();
    struct arch_percpu* arch_cpu = percpu_ref(arch_data);
    size_t slot;
    for (slot = 0; slot < countof(arch_cpu->irq_lines); slot++) {
        if (!arch_cpu->irq_lines[slot])
            break;
    }
    if (slot == countof(arch_cpu->irq_lines)) {
        irq_unlock();
        return ENOSPC;
    }

    *out_vector = slot + 0x20;
    *out_lapic = arch_cpu->lapic.id;
    arch_cpu->irq_lines[slot] = line;
    irq_unlock();
    return 0;
}

void apic_free_vector(uint8_t vector) {
    irq_lock();
    percpu_ref(arch_data)->irq_lines[vector - 0x20] = nullptr;
    irq_unlock();
}

errno_t arch_irq_alloc_msi(struct msi_line** out) {
    struct apic_msi_line* line = mem_alloc(sizeof(*line), 0);
    if (!line)
        return ENOMEM;

    line->msi.line.set_config = apic_msi_set_config;
    line->msi.line.mask = apic_msi_mask;
    line->msi.line.unmask = apic_msi_unmask;
    line->msi.line.eoi = apic_msi_eoi;
    line->msi.msg_addr = apic_msi_msg_addr;
    line->msi.msg_data = apic_msi_msg_data;

    errno_t status = apic_alloc_vector(&line->msi.line, &line->vector, &line->lapic);
    if (status) {
        mem_free(line, sizeof(*line));
        return status;
    }

    *out = &line->msi;
    return 0;
}
