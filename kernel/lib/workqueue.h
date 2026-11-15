// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/tailq.h>
#include <stdint.h>

struct work {
    void (*func)(struct work* work);
    tailq_link(struct work) link;
    atomic_bool pending;
};

struct workqueue;

// Workqueue for regular prios
extern struct workqueue* workqueue_system;
// Workqueue for high prios
extern struct workqueue* workqueue_high;

void work_init(struct work* work, void (*func)(struct work* work));

errno_t workqueue_create(const char* name, uint8_t priority, struct workqueue** out);

void workqueue_destroy(struct workqueue* wq);

bool work_queue(struct workqueue* wq, struct work* work);

bool work_cancel(struct workqueue* wq, struct work* work);

void workqueue_flush(struct workqueue* wq);

INIT_EXTERN(workqueue_stage);
