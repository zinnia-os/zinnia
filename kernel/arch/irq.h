// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/irq.h>

bool arch_irq_set_state(bool state);
bool arch_irq_get_state();
void arch_irq_wait();
errno_t arch_irq_alloc_msi(struct msi_line** out);
errno_t arch_irq_get_gsi(uint32_t gsi, struct irq_line** out);
bool arch_irq_isa_override(
    uint8_t isa_irq,
    uint32_t* out_gsi,
    enum irq_trigger_mode* out_trigger,
    enum irq_polarity* out_polarity
);
