#pragma once

// Busy-waits in a loop until the lock is freed.
// Does not put the CPU to sleep.
struct spinlock {
    bool locked;
};

// Attempts to lock a spinlock. If it's already locked, waits until it's freed.
void spin_lock(struct spinlock* spin);

// Unlocks a spinlock.
void spin_unlock(struct spinlock* spin);
