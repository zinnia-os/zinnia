// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/condvar.h>
#include <lib/park.h>
#include <stdint.h>

void condvar_init(struct condvar* cv) {
    atomic_store(&cv->seq, 0, ATOMIC_RELAXED);
    atomic_store(&cv->waiters, 0, ATOMIC_RELAXED);
}

void condvar_wait(struct condvar* cv, mutex_t* mutex) {
    atomic_fetch_add(&cv->waiters, 1, ATOMIC_SEQ_CST);
    uint32_t seq = atomic_load(&cv->seq, ATOMIC_SEQ_CST);
    mutex_unlock(mutex);
    park_wait(&cv->seq, seq);
    atomic_fetch_sub(&cv->waiters, 1, ATOMIC_RELAXED);
    mutex_lock(mutex);
}

static void wake(struct condvar* cv, size_t count) {
    atomic_fetch_add(&cv->seq, 1, ATOMIC_SEQ_CST);
    if (atomic_load(&cv->waiters, ATOMIC_SEQ_CST) != 0)
        park_wake(&cv->seq, count);
}

void condvar_signal(struct condvar* cv) {
    wake(cv, 1);
}

void condvar_broadcast(struct condvar* cv) {
    wake(cv, SIZE_MAX);
}
