// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/cpu.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <lib/spin.h>
#include <lib/timer.h>

struct timer_queue {
    struct spinlock lock;
    tailq_head(struct timer) timers;
};

[[__percpu]]
static struct timer_queue timer_queue;

void timer_init(struct timer* timer, void (*func)(struct timer* timer)) {
    timer->func = func;
    timer->deadline = 0;
    atomic_init(&timer->queue, nullptr);
    atomic_init(&timer->running, false);
}

void timer_cpu_init() {
    struct timer_queue* q = percpu_ref(timer_queue);
    spin_init(&q->lock);
    tailq_init(&q->timers);
}

static void program_next(struct timer_queue* q) {
    struct timer* first = tailq_first(&q->timers);
    if (first)
        arch_timer_program(first->deadline);
}

static void remove_locked(struct timer_queue* q, struct timer* timer) {
    tailq_remove(&q->timers, timer, link);
    atomic_store(&timer->queue, nullptr, ATOMIC_RELEASE);
}

static struct timer_queue* lock_queue_of(struct timer* timer) {
    while (true) {
        struct timer_queue* q = atomic_load(&timer->queue, ATOMIC_ACQUIRE);
        if (!q)
            return nullptr;
        spin_lock(&q->lock);
        if (atomic_load(&timer->queue, ATOMIC_RELAXED) == q)
            return q;
        spin_unlock(&q->lock);
    }
}

static bool dequeue(struct timer* timer) {
    struct timer_queue* q = lock_queue_of(timer);
    if (!q)
        return false;
    remove_locked(q, timer);
    spin_unlock(&q->lock);
    return true;
}

void timer_arm(struct timer* timer, uint64_t deadline_ns) {
    dequeue(timer);

    irq_lock();
    struct timer_queue* q = percpu_ref(timer_queue);
    spin_lock(&q->lock);

    timer->deadline = deadline_ns;
    struct timer* pos = tailq_first(&q->timers);
    while (pos && pos->deadline <= deadline_ns)
        pos = tailq_next(pos, link);
    if (pos)
        tailq_insert_before(pos, timer, link);
    else
        tailq_insert_tail(&q->timers, timer, link);
    atomic_store(&timer->queue, q, ATOMIC_RELEASE);

    if (tailq_first(&q->timers) == timer)
        arch_timer_program(deadline_ns);

    spin_unlock(&q->lock);
    irq_unlock();
}

bool timer_cancel(struct timer* timer) {
    bool removed = dequeue(timer);
    while (atomic_load(&timer->running, ATOMIC_ACQUIRE))
        cpu_pause();
    return removed;
}

void timer_interrupt() {
    struct timer_queue* q = percpu_ref(timer_queue);
    const uint64_t now = clock_get_elapsed_ns();

    spin_lock(&q->lock);
    struct timer* timer;
    while ((timer = tailq_first(&q->timers)) && timer->deadline <= now) {
        atomic_store(&timer->running, true, ATOMIC_RELAXED);
        remove_locked(q, timer);
        spin_unlock(&q->lock);

        timer->func(timer);
        atomic_store(&timer->running, false, ATOMIC_RELEASE);

        spin_lock(&q->lock);
    }
    program_next(q);
    spin_unlock(&q->lock);
}
