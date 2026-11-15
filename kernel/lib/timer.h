// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/tailq.h>
#include <stdint.h>

struct timer_queue;

struct timer {
    uint64_t deadline;
    void (*func)(struct timer* timer);
    _Atomic(struct timer_queue*) queue;
    atomic_bool running;
    tailq_link(struct timer) link;
};

void timer_init(struct timer* timer, void (*func)(struct timer* timer));
void timer_arm(struct timer* timer, uint64_t deadline_ns);
bool timer_cancel(struct timer* timer);

void timer_cpu_init();
void timer_interrupt();
