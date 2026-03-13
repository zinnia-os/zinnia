#include <kernel/irq.h>
#include <kernel/percpu.h>
#include <stdatomic.h>

void irq_lock() {
    struct irq_percpu* cpu = &percpu_get()->irq;
    if (atomic_load_explicit(&cpu->in_interrupt, memory_order_acquire))
        return;

    arch_irq_set_state(false);
    atomic_fetch_add_explicit(&percpu_get()->irq.level, 1, memory_order_acq_rel);
}

void irq_unlock() {
    struct irq_percpu* cpu = &percpu_get()->irq;
    if (atomic_load_explicit(&cpu->in_interrupt, memory_order_acquire))
        return;

    uint32_t old_level = atomic_fetch_sub_explicit(&cpu->level, 1, memory_order_acq_rel);
    // If it was 1, the new IRQ level is now 0.
    if (old_level == 1) {
        arch_irq_set_state(true);
    }
}

bool irq_set_interrupted(bool is_interrupted) {
    return atomic_exchange_explicit(&percpu_get()->irq.in_interrupt, is_interrupted, memory_order_release);
}
