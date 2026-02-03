#include <kernel/irq.h>
#include <kernel/percpu.h>
#include <stdatomic.h>

void irq_lock() {
    irq_set_state(false);
    atomic_fetch_add_explicit(&percpu_get()->irq.level, 1, memory_order_acq_rel);
}

void irq_unlock() {
    uint32_t old_level = atomic_fetch_sub_explicit(&percpu_get()->irq.level, 1, memory_order_acq_rel);
    // If it was 1, the new IRQ level is now 0.
    if (old_level == 1) {
        irq_set_state(true);
    }
}
