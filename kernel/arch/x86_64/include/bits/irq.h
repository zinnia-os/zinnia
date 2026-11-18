// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/x86_64/defs.h>
#include <lib/irq.h>
#include <stdint.h>

errno_t arch_irq_alloc_msi(struct msi_line** out);
errno_t arch_irq_get_gsi(uint32_t gsi, struct irq_line** out);
bool arch_irq_isa_override(
    uint8_t isa_irq,
    uint32_t* out_gsi,
    enum irq_trigger_mode* out_trigger,
    enum irq_polarity* out_polarity
);

static inline bool arch_irq_get_state() {
    uint64_t flags;
    asm volatile("pushf; pop %0" : "=r"(flags)::"memory");
    return flags & RFLAGS_IF;
}

static inline bool arch_irq_set_state(bool state) {
    bool old = arch_irq_get_state();
    if (state)
        asm volatile("sti" ::: "memory");
    else
        asm volatile("cli" ::: "memory");
    return old;
}

static inline void arch_irq_wait() {
    asm volatile("hlt");
}
