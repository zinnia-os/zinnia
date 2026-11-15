// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/park.h>
#include <lib/semaphore.h>

void sem_init(sem_t* sem, uint32_t count) {
    atomic_store(&sem->count, count, ATOMIC_RELAXED);
}

bool sem_trywait(sem_t* sem) {
    uint32_t count = atomic_load(&sem->count, ATOMIC_RELAXED);
    while (count) {
        if (atomic_compare_exchange_weak(&sem->count, &count, count - 1, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
            return true;
    }
    return false;
}

static bool sem_empty(void* ctx) {
    sem_t* sem = ctx;
    return atomic_load(&sem->count, ATOMIC_RELAXED) == 0;
}

bool sem_timedwait(sem_t* sem, const uint64_t* deadline_ns) {
    while (!sem_trywait(sem)) {
        if (park_timeout(sem, sem_empty, nullptr, sem, 0, deadline_ns).status == PARK_TIMED_OUT)
            return sem_trywait(sem);
    }
    return true;
}

void sem_wait(sem_t* sem) {
    sem_timedwait(sem, nullptr);
}

void sem_post(sem_t* sem) {
    atomic_fetch_add(&sem->count, 1, ATOMIC_RELEASE);
    unpark_one(sem, nullptr, nullptr);
}
