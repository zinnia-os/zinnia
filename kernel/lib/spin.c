// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/cpu.h>
#include <lib/irq.h>
#include <lib/spin.h>

void spin_init(struct spinlock* spin) {
    spin->next = 0;
    spin->owner = 0;
}

void spin_lock(struct spinlock* spin) {
    irq_lock();
    uint32_t my = atomic_fetch_add(&spin->next, 1, ATOMIC_RELAXED);

    while (atomic_load(&spin->owner, ATOMIC_ACQUIRE) != my) {
        cpu_pause();
    }
}

bool spin_trylock(struct spinlock* spin) {
    irq_lock();
    uint32_t owner = atomic_load(&spin->owner, ATOMIC_RELAXED);
    uint32_t expected = owner;
    if (atomic_compare_exchange(&spin->next, &expected, owner + 1, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
        return true;
    irq_unlock();
    return false;
}

void spin_unlock(struct spinlock* spin) {
    uint32_t val = atomic_load(&spin->owner, ATOMIC_RELAXED);
    atomic_store(&spin->owner, val + 1, ATOMIC_RELEASE);
    irq_unlock();
}
