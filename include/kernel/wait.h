#pragma once

#include <kernel/spin.h>
#include <kernel/tailq.h>

struct task;

// A queue of tasks waiting for an event.
struct wait_queue {
    TAILQ_HEAD(struct task) waiters;
    struct spinlock lock;
};

void wait_queue_init(struct wait_queue* wq);
