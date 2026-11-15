// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <stddef.h>
#include <stdint.h>

struct task;

enum park_status {
    PARK_UNPARKED,
    PARK_INVALID,
    PARK_TIMED_OUT,
    PARK_INTERRUPTED,
};

struct park_result {
    enum park_status status;
    uintptr_t token;
};

enum park_flags {
    // Queue in front of waiters with the same priority.
    PARK_FRONT = 1 << 0,
    PARK_INTERRUPTIBLE = 1 << 1,
};

struct park_unpark_result {
    struct task* task; // The task that is being woken up, or nullptr if there was no waiter.
    bool have_more;    // Whether there are more tasks parked on the same key.
};

// Parks the current task on `key` if `validate(ctx)` returns true.
// `validate` is called with the queue for `key` locked, so it must be short and must not sleep.
// Returns the token passed by the waker, or `PARK_INVALID` if `validate` returned false.
// Must not be called while holding a spinlock.
struct park_result park(const void* key, bool (*validate)(void* ctx), void* ctx, enum park_flags flags);

// Very similar to `park`, but with an optional `deadline_ns`.
// If the deadline is missed, `cancelled` gets called (also optional).
struct park_result park_timeout(
    const void* key,
    bool (*validate)(void* ctx),
    void (*cancelled)(void* ctx, bool was_last),
    void* ctx,
    enum park_flags flags,
    const uint64_t* deadline_ns
);

// Wakes the first task parked on `key`.
// `callback`, if set, is called with the queue locked before the task is woken, even if there was no waiter.
// Its return value is passed to the woken task as the token. Without a callback, the token is 0.
// Returns true if a task was woken.
bool unpark_one(const void* key, uintptr_t (*callback)(void* ctx, const struct park_unpark_result* result), void* ctx);

// Wakes all tasks parked on `key` and passes `token` to them. Returns the amount of tasks woken.
size_t unpark_all(const void* key, uintptr_t token);

// Parks the current task as long as `*addr == expected`. Returns false if the value didn't match.
bool park_wait(atomic_uint* addr, unsigned int expected);

// Wakes up to `count` tasks waiting in park_wait() on `addr`. Returns the amount of tasks woken.
size_t park_wake(atomic_uint* addr, size_t count);
