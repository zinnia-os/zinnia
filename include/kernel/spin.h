#pragma once

#include <stdint.h>

// Busy-waits in a loop until the lock is freed.
// Does not put the CPU to sleep.
struct spinlock {
    uint32_t locked;
};

// Attempts to lock a spinlock. If it's already locked, waits until it's freed.
void spin_lock(struct spinlock* spin);

// Unlocks a spinlock.
void spin_unlock(struct spinlock* spin);
