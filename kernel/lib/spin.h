// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

// Busy-waits in a loop until the lock is freed.
// Does not put the CPU to sleep.
struct spinlock {
    _Atomic(uint32_t) owner;
    _Atomic(uint32_t) next;
};

void spin_init(struct spinlock* spin);

// Attempts to lock a spinlock. If it's already locked, waits until it's freed.
void spin_lock(struct spinlock* spin);

bool spin_trylock(struct spinlock* spin);

// Unlocks a spinlock.
void spin_unlock(struct spinlock* spin);
