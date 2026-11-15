// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/tailq.h>
#include <stdint.h>

struct task;
struct turnstile_chain;

enum turnstile_queue {
    TURNSTILE_EXCLUSIVE,
    TURNSTILE_SHARED,

    TURNSTILE_NUM_QUEUES,
};

struct turnstile {
    // The lock object that this turnstile is assigned to.
    // If not in use, it's set to `nullptr`.
    const void* obj;

    // The task that inherits the priority from the waiters.
    struct task* owner;
    bool lending;

    // Priority of the most urgent waiter in any queue.
    _Atomic(uint8_t) top_priority;

    tailq_head(struct task) waiters[TURNSTILE_NUM_QUEUES];

    // Spare turnstiles donated by waiters.
    struct turnstile* spare;

    // Next active turnstile in the hash chain.
    struct turnstile* chain_next;

    // Link for `task::pi_lenders`.
    tailq_link(struct turnstile) lender_link;
};

struct turnstile* turnstile_alloc();
void turnstile_free(struct turnstile* ts);

struct turnstile_chain* turnstile_lock(const void* obj);
void turnstile_unlock(struct turnstile_chain* chain);

void turnstile_block(struct turnstile_chain* chain, const void* obj, struct task* owner, enum turnstile_queue queue);

struct task* turnstile_first(
    struct turnstile_chain* chain,
    const void* obj,
    enum turnstile_queue queue,
    bool* have_more
);

void turnstile_wake(struct turnstile_chain* chain, const void* obj, struct task* task, struct task* new_owner);

void turnstile_waive(struct turnstile_chain* chain, const void* obj);

void turnstile_reprioritize(struct task* task);
