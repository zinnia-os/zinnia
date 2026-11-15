// SPDX-License-Identifier: BSD-3-Clause

#include <fs/poll.h>
#include <lib/atomic.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/timer.h>
#include <mem/alloc.h>

struct poll_timeout {
    struct timer timer;
    struct task* task;
    atomic_bool expired;
};

void poll_event_init(struct poll_event* event) {
    spin_init(&event->lock);
    tailq_init(&event->waiters);
}

void poll_event_notify(struct poll_event* event) {
    spin_lock(&event->lock);
    for (struct poll_waiter* w = tailq_first(&event->waiters); w; w = tailq_next(w, link)) {
        if (w->table->notify) {
            w->table->notify(w->table);
            continue;
        }
        atomic_store(&w->table->triggered, true, ATOMIC_RELEASE);
        sched_wake(w->table->task);
    }
    spin_unlock(&event->lock);
}

void poll_table_init(struct poll_table* table) {
    table->task = percpu_get()->sched.current;
    table->notify = nullptr;
    atomic_init(&table->triggered, false);
    table->waiters = nullptr;
    table->inline_used = false;
    table->interruptible = true;
    table->error = 0;
}

void poll_table_init_callback(struct poll_table* table, void (*notify)(struct poll_table* table)) {
    poll_table_init(table);
    table->task = nullptr;
    table->notify = notify;
}

void poll_table_fini(struct poll_table* table) {
    struct poll_waiter* w = table->waiters;
    while (w) {
        struct poll_waiter* next = w->next;
        spin_lock(&w->event->lock);
        tailq_remove(&w->event->waiters, w, link);
        spin_unlock(&w->event->lock);
        if (w != &table->inline_waiter)
            mem_free(w, sizeof(*w));
        w = next;
    }
    table->waiters = nullptr;
    table->inline_used = false;
}

void poll_wait(struct poll_table* table, struct poll_event* event) {
    if (!table)
        return;

    struct poll_waiter* w;
    if (!table->inline_used) {
        w = &table->inline_waiter;
        table->inline_used = true;
    } else if (!(w = mem_alloc(sizeof(*w), 0))) {
        table->error = ENOMEM;
        return;
    }
    w->event = event;
    w->table = table;
    w->next = table->waiters;
    table->waiters = w;

    spin_lock(&event->lock);
    tailq_insert_tail(&event->waiters, w, link);
    spin_unlock(&event->lock);
}

static void timeout_fn(struct timer* timer) {
    struct poll_timeout* timeout = containerof(timer, struct poll_timeout, timer);
    atomic_store(&timeout->expired, true, ATOMIC_RELEASE);
    sched_wake(timeout->task);
}

errno_t poll_table_wait(struct poll_table* table, const uint64_t* deadline_ns) {
    struct poll_timeout timeout = {.task = table->task};
    atomic_init(&timeout.expired, false);
    timer_init(&timeout.timer, timeout_fn);
    if (deadline_ns)
        timer_arm(&timeout.timer, *deadline_ns);

    errno_t status = 0;
    while (!atomic_load(&table->triggered, ATOMIC_ACQUIRE)) {
        if (atomic_load(&timeout.expired, ATOMIC_ACQUIRE)) {
            status = ETIMEDOUT;
            break;
        }
        if (table->interruptible && signal_pending(table->task)) {
            status = EINTR;
            break;
        }
        sched_block(&percpu_get()->sched);
    }

    if (deadline_ns)
        timer_cancel(&timeout.timer);
    return status;
}

errno_t poll_table_sleep(struct poll_table* table, mutex_t* lock, const uint64_t* deadline_ns) {
    mutex_unlock(lock);
    errno_t status = poll_table_wait(table, deadline_ns);
    mutex_lock(lock);
    atomic_store(&table->triggered, false, ATOMIC_RELAXED);
    return status;
}
