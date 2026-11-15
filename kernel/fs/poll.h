// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/mutex.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <stdint.h>
#include <zinnia/poll.h>

enum poll_flags : uint16_t {
    POLLIN = __POLLIN,
    POLLOUT = __POLLOUT,
    POLLPRI = __POLLPRI,
    POLLHUP = __POLLHUP,
    POLLERR = __POLLERR,
    POLLRDHUP = __POLLRDHUP,
    POLLNVAL = __POLLNVAL,
    POLLRDNORM = __POLLRDNORM,
    POLLRDBAND = __POLLRDBAND,
    POLLWRNORM = __POLLWRNORM,
    POLLWRBAND = __POLLWRBAND,
};

struct task;
struct poll_table;
struct poll_event;

struct poll_waiter {
    struct poll_event* event;
    struct poll_table* table;
    tailq_link(struct poll_waiter) link;
    struct poll_waiter* next;
};

struct poll_event {
    struct spinlock lock;
    tailq_head(struct poll_waiter) waiters;
};

struct poll_table {
    struct task* task;
    void (*notify)(struct poll_table* table);
    atomic_bool triggered;
    struct poll_waiter* waiters;
    struct poll_waiter inline_waiter;
    bool inline_used;
    bool interruptible;
    errno_t error;
};

void poll_event_init(struct poll_event* event);
void poll_event_notify(struct poll_event* event);

void poll_table_init(struct poll_table* table);
void poll_table_init_callback(struct poll_table* table, void (*notify)(struct poll_table* table));
void poll_table_fini(struct poll_table* table);
void poll_wait(struct poll_table* table, struct poll_event* event);
errno_t poll_table_wait(struct poll_table* table, const uint64_t* deadline_ns);
errno_t poll_table_sleep(struct poll_table* table, mutex_t* lock, const uint64_t* deadline_ns);
