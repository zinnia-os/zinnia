// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/initgraph.h>
#include <stdint.h>

struct epoch_head {
    struct epoch_head* next;
    void (*func)(struct epoch_head* head);
};

// Enters a read-side critical section.
// Disables preemption, so the caller must not sleep.
// A reader may call this function even while already in an epoch section.
void epoch_enter();

// Leaves a read-side critical section.
void epoch_exit();

// Returns true if the current CPU is inside a critical section.
bool epoch_active();

// Runs `func` once all readers that could observe `head` have left.
void epoch_call(struct epoch_head* head, void (*func)(struct epoch_head* head));

// Advances the global epoch if all active readers have observed it.
bool epoch_try_advance();

// Returns the current global epoch.
uint64_t epoch_current();

// Waits until all readers active at the time of the call have left.
void epoch_synchronize();

// Waits until all previously queued callbacks have run.
void epoch_barrier();

// Loads a pointer protected by an epoch critical section.
#define epoch_deref(ptr) atomic_load(ptr, ATOMIC_ACQUIRE)

// Publishes a pointer to epoch readers.
#define epoch_publish(ptr, val) atomic_store(ptr, val, ATOMIC_RELEASE)

INIT_EXTERN(epoch_stage);
