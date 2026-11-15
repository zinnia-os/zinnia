// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/irq.h>
#include <lib/park.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/spin.h>
#include <lib/timer.h>
#include <lib/utils.h>

constexpr size_t PARK_BUCKET_BITS = 8;
constexpr size_t PARK_BUCKETS = (1 << PARK_BUCKET_BITS);

enum park_waiter_state : unsigned int {
    WAITER_QUEUED,
    WAITER_CLAIMED,
    WAITER_DONE,
};

struct park_waiter {
    const void* key;
    struct task* task;
    uint8_t priority;
    atomic_uint state;
    struct park_result result;
    struct park_waiter* prev;
    struct park_waiter* next;
    struct park_waiter* key_prev;
    struct park_waiter* key_next;
    struct park_waiter* wake_next;
};

struct park_bucket {
    alignas(64) struct spinlock lock;
    struct park_waiter* keys;
};

static struct park_bucket buckets[PARK_BUCKETS];

static struct park_bucket* bucket_for(const void* key) {
    uint64_t hash = ((uintptr_t)key >> 3) * 0x9E3779B97F4A7C15ull;
    return &buckets[hash >> (64 - PARK_BUCKET_BITS)];
}

static struct park_waiter* queue_find(struct park_bucket* b, const void* key) {
    struct park_waiter* head = b->keys;
    while (head && head->key != key)
        head = head->key_next;
    return head;
}

static void queue_replace_head(struct park_bucket* b, struct park_waiter* old, struct park_waiter* new) {
    new->key_prev = old->key_prev;
    new->key_next = old->key_next;
    if (new->key_prev)
        new->key_prev->key_next = new;
    else
        b->keys = new;
    if (new->key_next)
        new->key_next->key_prev = new;
}

// Queues `w` behind all waiters on the same key that are at least as urgent.
// With `front`, it's queued in front of waiters on the same key with the same priority instead.
// The priority is only considered at the time of queueing.
static void queue_insert(struct park_bucket* b, struct park_waiter* w, bool front) {
    struct park_waiter* head = queue_find(b, w->key);
    if (!head) {
        w->prev = nullptr;
        w->next = nullptr;
        w->key_prev = nullptr;
        w->key_next = b->keys;
        if (b->keys)
            b->keys->key_prev = w;
        b->keys = w;
        return;
    }

    struct park_waiter* last = nullptr;
    struct park_waiter* pos = head;
    while (pos && (pos->priority > w->priority || (!front && pos->priority == w->priority))) {
        last = pos;
        pos = pos->next;
    }

    w->prev = last;
    w->next = pos;
    if (pos)
        pos->prev = w;
    if (last)
        last->next = w;
    else
        queue_replace_head(b, head, w);
}

static void queue_remove(struct park_bucket* b, struct park_waiter* w) {
    if (w->next)
        w->next->prev = w->prev;
    if (w->prev) {
        w->prev->next = w->next;
        return;
    }
    if (w->next) {
        queue_replace_head(b, w, w->next);
        return;
    }
    if (w->key_prev)
        w->key_prev->key_next = w->key_next;
    else
        b->keys = w->key_next;
    if (w->key_next)
        w->key_next->key_prev = w->key_prev;
}

// Must be called with the bucket locksd. `w` must already be removed from the bucket.
static void claim(struct park_waiter* w, uintptr_t token) {
    w->result = (struct park_result){.status = PARK_UNPARKED, .token = token};
    w->wake_next = nullptr;
    atomic_store(&w->state, WAITER_CLAIMED, ATOMIC_RELAXED);
}

static void wake_claimed(struct park_waiter* w) {
    while (w) {
        struct park_waiter* next = w->wake_next;
        struct task* task = w->task;
        task_lock(task);
        atomic_store(&w->state, WAITER_DONE, ATOMIC_RELEASE);
        sched_wake_locked(task);
        task_unlock(task);
        w = next;
    }
}

static bool cancel(
    struct park_bucket* b,
    struct park_waiter* w,
    enum park_status status,
    void (*cancelled)(void* ctx, bool was_last),
    void* ctx
) {
    spin_lock(&b->lock);
    const bool queued = atomic_load(&w->state, ATOMIC_RELAXED) == WAITER_QUEUED;
    if (queued) {
        queue_remove(b, w);
        if (cancelled)
            cancelled(ctx, queue_find(b, w->key) == nullptr);
        w->result = (struct park_result){.status = status};
        atomic_store(&w->state, WAITER_DONE, ATOMIC_RELAXED);
    }
    spin_unlock(&b->lock);
    return queued;
}

struct park_timer {
    struct timer timer;
    struct task* task;
    atomic_bool expired;
};

static void park_timer_fn(struct timer* timer) {
    struct park_timer* t = containerof(timer, struct park_timer, timer);
    atomic_store(&t->expired, true, ATOMIC_RELEASE);
    sched_wake(t->task);
}

struct park_result park_timeout(
    const void* key,
    bool (*validate)(void* ctx),
    void (*cancelled)(void* ctx, bool was_last),
    void* ctx,
    enum park_flags flags,
    const uint64_t* deadline_ns
) {
    struct task* self = percpu_get()->sched.current;
    assert(self != nullptr, "Tried to park without a scheduler");
    assert(irq_get_state(), "Tried to park with interrupts disabled");

    struct park_waiter w = {
        .key = key,
        .task = self,
        .priority = atomic_load(&self->priority, ATOMIC_RELAXED),
    };
    atomic_init(&w.state, WAITER_QUEUED);

    struct park_bucket* b = bucket_for(key);
    spin_lock(&b->lock);
    if (validate && !validate(ctx)) {
        spin_unlock(&b->lock);
        return (struct park_result){.status = PARK_INVALID};
    }
    if (deadline_ns && clock_get_elapsed_ns() >= *deadline_ns) {
        if (cancelled)
            cancelled(ctx, queue_find(b, key) == nullptr);
        spin_unlock(&b->lock);
        return (struct park_result){.status = PARK_TIMED_OUT};
    }
    queue_insert(b, &w, flags & PARK_FRONT);
    spin_unlock(&b->lock);

    struct park_timer timer = {.task = self};
    atomic_init(&timer.expired, false);
    if (deadline_ns) {
        timer_init(&timer.timer, park_timer_fn);
        timer_arm(&timer.timer, *deadline_ns);
    }

    // sched_block() can return early, so check whether we were actually unparked.
    // The task may migrate while blocked, the scheduler has to be looked up again every time.
    while (atomic_load(&w.state, ATOMIC_ACQUIRE) != WAITER_DONE) {
        enum park_status reason = PARK_UNPARKED;
        if (atomic_load(&timer.expired, ATOMIC_ACQUIRE))
            reason = PARK_TIMED_OUT;
        else if ((flags & PARK_INTERRUPTIBLE) && signal_pending(self))
            reason = PARK_INTERRUPTED;
        if (reason != PARK_UNPARKED && cancel(b, &w, reason, cancelled, ctx))
            break;
        sched_block(&percpu_get()->sched);
    }

    if (deadline_ns)
        timer_cancel(&timer.timer);
    task_lock(self);
    task_unlock(self);
    return w.result;
}

struct park_result park(const void* key, bool (*validate)(void* ctx), void* ctx, enum park_flags flags) {
    return park_timeout(key, validate, nullptr, ctx, flags, nullptr);
}

bool unpark_one(const void* key, uintptr_t (*callback)(void* ctx, const struct park_unpark_result* result), void* ctx) {
    struct park_bucket* b = bucket_for(key);
    spin_lock(&b->lock);

    struct park_waiter* w = queue_find(b, key);
    if (w)
        queue_remove(b, w);

    struct park_unpark_result result = {
        .task = w ? w->task : nullptr,
        .have_more = queue_find(b, key) != nullptr,
    };
    uintptr_t token = callback ? callback(ctx, &result) : 0;

    if (w)
        claim(w, token);

    spin_unlock(&b->lock);
    wake_claimed(w);
    return w != nullptr;
}

static size_t unpark_n(const void* key, size_t count, uintptr_t token) {
    struct park_bucket* b = bucket_for(key);
    struct park_waiter* list = nullptr;
    struct park_waiter** tail = &list;
    size_t woken = 0;

    spin_lock(&b->lock);
    struct park_waiter* w;
    while (woken < count && (w = queue_find(b, key))) {
        queue_remove(b, w);
        claim(w, token);
        *tail = w;
        tail = &w->wake_next;
        woken++;
    }
    spin_unlock(&b->lock);

    wake_claimed(list);
    return woken;
}

size_t unpark_all(const void* key, uintptr_t token) {
    return unpark_n(key, SIZE_MAX, token);
}

struct park_wait_ctx {
    atomic_uint* addr;
    unsigned int expected;
};

static bool park_wait_validate(void* ctx) {
    struct park_wait_ctx* c = ctx;
    return atomic_load(c->addr, ATOMIC_RELAXED) == c->expected;
}

bool park_wait(atomic_uint* addr, unsigned int expected) {
    struct park_wait_ctx ctx = {addr, expected};
    return park(addr, park_wait_validate, &ctx, 0).status != PARK_INVALID;
}

size_t park_wake(atomic_uint* addr, size_t count) {
    return unpark_n(addr, count, 0);
}
