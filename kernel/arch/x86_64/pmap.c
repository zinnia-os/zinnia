// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/percpu.h>
#include <arch/x86_64/pmap.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/types.h>
#include <mem/pmap.h>

void arch_pte_shootdown_kick(struct percpu* cpu) {
    irq_lock();
    lapic_send_ipi(
        &percpu_ref(arch_data)->lapic,
        percpu_ref_on(cpu, arch_data)->lapic.id,
        IDT_IPI_SHOOTDOWN,
        APIC_DELIVERY_FIXED,
        APIC_DEST_PHYSICAL,
        APIC_LEVEL_ASSERT,
        APIC_TRIGGER_EDGE
    );
    irq_unlock();
}
