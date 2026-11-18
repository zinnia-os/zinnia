// SPDX-License-Identifier: BSD-3-Clause

#include <fs/poll.h>
#include <lib/atomic.h>
#include <lib/futex.h>
#include <lib/mutex.h>
#include <lib/sched.h>
#include <lib/tailq.h>
#include <lib/utils.h>
#include <mem/usercopy.h>

struct futex_waiter {
    struct vma* space;
    uintptr_t addr;
    struct poll_table table;
    bool queued;
    tailq_link(struct futex_waiter) link;
};

struct futex_bucket {
    mutex_t lock;
    tailq_head(struct futex_waiter) waiters;
};

constexpr size_t FUTEX_BUCKETS = 64;

static struct futex_bucket buckets[FUTEX_BUCKETS];

static struct futex_bucket* bucket_of(struct vma* space, uintptr_t addr) {
    uint64_t hash = ((uintptr_t)space >> 4) ^ (addr >> 2);
    hash *= 0x9E37'79B9'7F4A'7C15;
    return &buckets[hash >> 58];
}

errno_t futex_wait(struct vma* space, uintptr_t addr, int expected, const uint64_t* deadline_ns) {
    if (addr % sizeof(int))
        return EINVAL;

    struct futex_bucket* bucket = bucket_of(space, addr);
    mutex_lock(&bucket->lock);
    if (!bucket->waiters.tqh_last)
        tailq_init(&bucket->waiters);

    int value;
    if (!usercopy_read(&value, (const __user void*)addr, sizeof(value))) {
        mutex_unlock(&bucket->lock);
        return EFAULT;
    }
    if (value != expected) {
        mutex_unlock(&bucket->lock);
        return EAGAIN;
    }

    struct futex_waiter waiter = {.space = space, .addr = addr, .queued = true};
    poll_table_init(&waiter.table);
    tailq_insert_tail(&bucket->waiters, &waiter, link);
    mutex_unlock(&bucket->lock);

    errno_t status = poll_table_wait(&waiter.table, deadline_ns);

    mutex_lock(&bucket->lock);
    const bool woken = !waiter.queued;
    if (waiter.queued)
        tailq_remove(&bucket->waiters, &waiter, link);
    mutex_unlock(&bucket->lock);

    return woken ? 0 : status;
}

size_t futex_wake(struct vma* space, uintptr_t addr, size_t count) {
    struct futex_bucket* bucket = bucket_of(space, addr);
    size_t woken = 0;

    mutex_lock(&bucket->lock);
    if (!bucket->waiters.tqh_last)
        tailq_init(&bucket->waiters);

    struct futex_waiter* waiter = tailq_first(&bucket->waiters);
    while (waiter && woken < count) {
        struct futex_waiter* next = tailq_next(waiter, link);
        if (waiter->space == space && waiter->addr == addr) {
            tailq_remove(&bucket->waiters, waiter, link);
            waiter->queued = false;
            atomic_store(&waiter->table.triggered, true, ATOMIC_RELEASE);
            sched_wake(waiter->table.task);
            woken++;
        }
        waiter = next;
    }
    mutex_unlock(&bucket->lock);
    return woken;
}
