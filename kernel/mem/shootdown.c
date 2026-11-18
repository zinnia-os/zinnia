// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/cpu.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <mem/pmap.h>

[[__percpu]]
static atomic_size_t shootdown_req;

[[__percpu]]
static atomic_size_t shootdown_ack;

void pmap_shootdown_handle() {
    const size_t req = atomic_load(percpu_ref(shootdown_req), ATOMIC_ACQUIRE);
    if (atomic_load(percpu_ref(shootdown_ack), ATOMIC_RELAXED) == req)
        return;
    pte_flush_all();
    atomic_store(percpu_ref(shootdown_ack), req, ATOMIC_RELEASE);
}

void pmap_shootdown(struct pmap* pt) {
    const size_t count = percpu_count();
    if (count < 2)
        return;

    irq_lock();
    struct percpu* self = percpu_get();
    for (size_t i = 0; i < count; i++) {
        struct percpu* cpu = percpu_cpu(i);
        if (cpu == self || !cpu->online)
            continue;
        atomic_fetch_add(percpu_ref_on(cpu, shootdown_req), 1, ATOMIC_RELEASE);
        pte_shootdown_kick(cpu);
    }

    for (size_t i = 0; i < count; i++) {
        struct percpu* cpu = percpu_cpu(i);
        if (cpu == self || !cpu->online)
            continue;
        const size_t req = atomic_load(percpu_ref_on(cpu, shootdown_req), ATOMIC_RELAXED);
        while ((ptrdiff_t)(req - atomic_load(percpu_ref_on(cpu, shootdown_ack), ATOMIC_ACQUIRE)) > 0) {
            pmap_shootdown_handle();
            cpu_pause();
        }
    }
    irq_unlock();
}
