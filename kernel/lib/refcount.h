// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/utils.h>
#include <stddef.h>

struct refcount {
    _Atomic(size_t) count;
};

static inline void refcount_init(struct refcount* rc) {
    atomic_store(&rc->count, 1, ATOMIC_RELAXED);
}

static inline void refcount_inc(struct refcount* rc) {
    atomic_fetch_add(&rc->count, 1, ATOMIC_RELAXED);
}

static inline bool refcount_inc_not_zero(struct refcount* rc) {
    size_t count = atomic_load(&rc->count, ATOMIC_RELAXED);
    do {
        if (count == 0)
            return false;
    } while (!atomic_compare_exchange_weak(&rc->count, &count, count + 1, ATOMIC_ACQUIRE, ATOMIC_RELAXED));
    return true;
}

static inline bool refcount_dec(struct refcount* rc) {
    if (atomic_fetch_sub(&rc->count, 1, ATOMIC_RELEASE) != 1)
        return false;
    atomic_fence(ATOMIC_ACQUIRE);
    return true;
}
