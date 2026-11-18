// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/cpu.h>
#include <lib/epoch.h>
#include <lib/irq.h>
#include <lib/mutex.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/spin.h>
#include <lib/timer.h>

constexpr uint64_t EPOCH_NEST_BITS = 16;
constexpr uint64_t EPOCH_NEST_MASK = ((uint64_t)1 << EPOCH_NEST_BITS) - 1;
constexpr size_t EPOCH_BUCKETS = 3;
constexpr size_t EPOCH_BATCH = 64;
constexpr size_t EPOCH_SYNC_SPINS = 64;
constexpr uint64_t EPOCH_POLL_NS = 10'000'000;
constexpr uint64_t EPOCH_SYNC_POLL_NS = 1'000'000;

struct epoch_list {
    struct epoch_head* head;
    struct epoch_head* tail;
};

struct epoch_bucket {
    uint64_t epoch;
    struct epoch_list list;
};

struct epoch_cpu {
    _Atomic(uint64_t) local;
    struct spinlock lock;
    struct epoch_bucket buckets[EPOCH_BUCKETS];
    struct epoch_list ready;
};

[[__percpu]]
static struct epoch_cpu epoch_data;

static _Atomic(uint64_t) global_epoch = 0;
static atomic_size_t pending = 0;
static mutex_t reclaim_lock;
static struct timer poll_timer;
static _Atomic(struct task*) reclaimer = nullptr;
static atomic_bool reclaimer_idle = false;

void epoch_enter() {
    sched_preempt_disable();
    _Atomic(uint64_t)* local = &percpu_ref(epoch_data)->local;
    const uint64_t value = atomic_load(local, ATOMIC_RELAXED);

    // If we already entered the critical section before, just increment the nest counter.
    if (value & EPOCH_NEST_MASK) {
        assert((value & EPOCH_NEST_MASK) != EPOCH_NEST_MASK, "Epoch nesting overflow");
        atomic_store(local, value + 1, ATOMIC_RELAXED);
        return;
    }

    // Store the current epoch in the upper bits, and mark this CPU as active.
    atomic_store(local, (atomic_load(&global_epoch, ATOMIC_RELAXED) << EPOCH_NEST_BITS) | 1, ATOMIC_RELAXED);
    atomic_fence(ATOMIC_SEQ_CST);
}

void epoch_exit() {
    _Atomic(uint64_t)* local = &percpu_ref(epoch_data)->local;
    const uint64_t value = atomic_load(local, ATOMIC_RELAXED);
    assert(value & EPOCH_NEST_MASK, "Tried to exit an epoch section without entering one");

    // If we didn't nest, we can clear the local epoch.
    // Otherwise, decrease the nest counter.
    if ((value & EPOCH_NEST_MASK) == 1)
        atomic_store(local, 0, ATOMIC_RELEASE);
    else
        atomic_store(local, value - 1, ATOMIC_RELAXED);

    sched_preempt_enable();
}

bool epoch_active() {
    sched_preempt_disable();
    const bool active = atomic_load(&percpu_ref(epoch_data)->local, ATOMIC_RELAXED) & EPOCH_NEST_MASK;
    sched_preempt_enable();
    return active;
}

static void list_push(struct epoch_list* list, struct epoch_head* head) {
    head->next = list->head;
    if (!list->head)
        list->tail = head;
    list->head = head;
}

static void list_splice(struct epoch_list* dst, struct epoch_list* src) {
    if (!src->head)
        return;
    src->tail->next = dst->head;
    if (!dst->head)
        dst->tail = src->tail;
    dst->head = src->head;
    *src = (struct epoch_list){};
}

static void wake_reclaimer() {
    struct task* task = atomic_load(&reclaimer, ATOMIC_ACQUIRE);
    if (task && atomic_exchange(&reclaimer_idle, false, ATOMIC_SEQ_CST))
        sched_wake(task);
}

void epoch_call(struct epoch_head* head, void (*func)(struct epoch_head* head)) {
    head->func = func;
    const size_t old = atomic_fetch_add(&pending, 1, ATOMIC_RELAXED);
    atomic_fence(ATOMIC_SEQ_CST);

    irq_lock();
    struct epoch_cpu* cpu = percpu_ref(epoch_data);
    spin_lock(&cpu->lock);

    // Store the object head in the bucket of this epoch.
    // We only need to store 3 buckets, since only these epochs are relevant:
    // - Current epoch, may have readers
    // - Previous epoch, may still have readers
    // - Safe epoch, has no readers remaining
    const uint64_t now = atomic_load(&global_epoch, ATOMIC_RELAXED);
    struct epoch_bucket* bucket = &cpu->buckets[now % EPOCH_BUCKETS];
    if (bucket->epoch != now) {
        // Combine both epoch lists.
        list_splice(&cpu->ready, &bucket->list);
        bucket->epoch = now;
    }
    list_push(&bucket->list, head);

    spin_unlock(&cpu->lock);
    irq_unlock();

    if (old == 0 || old % EPOCH_BATCH == 0)
        wake_reclaimer();
}

uint64_t epoch_current() {
    return atomic_load(&global_epoch, ATOMIC_RELAXED);
}

bool epoch_try_advance() {
    uint64_t now = atomic_load(&global_epoch, ATOMIC_RELAXED);
    atomic_fence(ATOMIC_SEQ_CST);
    const size_t count = percpu_count();

    // Go through all CPUs.
    for (size_t i = 0; i < count; i++) {
        const uint64_t local = atomic_load(&percpu_ref_on(percpu_cpu(i), epoch_data)->local, ATOMIC_ACQUIRE);

        // If any of the CPUs are in a critical section, and their local epoch
        // is not the same as the current global epoch, we can't advance.
        if ((local & EPOCH_NEST_MASK) && (local >> EPOCH_NEST_BITS) != now)
            return false;
    }

    // At this point all readers agree on the epoch, it's safe to increment now.
    atomic_compare_exchange(&global_epoch, &now, now + 1, ATOMIC_SEQ_CST, ATOMIC_RELAXED);
    return true;
}

static struct epoch_list collect(struct epoch_cpu* cpu, uint64_t now) {
    spin_lock(&cpu->lock);
    for (size_t i = 0; i < EPOCH_BUCKETS; i++) {
        struct epoch_bucket* bucket = &cpu->buckets[i];
        if (bucket->epoch + 2 <= now)
            list_splice(&cpu->ready, &bucket->list);
    }
    struct epoch_list out = cpu->ready;
    cpu->ready = (struct epoch_list){};
    spin_unlock(&cpu->lock);
    return out;
}

static void run(struct epoch_head* head) {
    size_t count = 0;
    while (head) {
        struct epoch_head* next = head->next;
        head->func(head);
        head = next;
        count++;
    }
    if (count)
        atomic_fetch_sub(&pending, count, ATOMIC_RELAXED);
}

static void reclaim() {
    mutex_lock(&reclaim_lock);
    const uint64_t now = atomic_load(&global_epoch, ATOMIC_ACQUIRE);
    const size_t count = percpu_count();
    for (size_t i = 0; i < count; i++)
        run(collect(percpu_ref_on(percpu_cpu(i), epoch_data), now).head);
    mutex_unlock(&reclaim_lock);
}

void epoch_synchronize() {
    // We can't synchronize a write while being a reader ourselves.
    assert(!epoch_active(), "Tried to synchronize inside an epoch section");

    atomic_fence(ATOMIC_SEQ_CST);
    const uint64_t target = atomic_load(&global_epoch, ATOMIC_RELAXED) + 2;
    for (size_t spins = 0; atomic_load(&global_epoch, ATOMIC_ACQUIRE) < target; spins++) {
        if (epoch_try_advance())
            continue;
        if (spins < EPOCH_SYNC_SPINS)
            cpu_pause();
        else
            sched_sleep_until(clock_get_elapsed_ns() + EPOCH_SYNC_POLL_NS);
    }
}

void epoch_barrier() {
    epoch_synchronize();
    reclaim();
}

static void poll_tick(struct timer*) {
    wake_reclaimer();
}

static void reclaimer_fn(uintptr_t) {
    while (true) {
        if (atomic_load(&pending, ATOMIC_RELAXED)) {
            epoch_try_advance();
            reclaim();
        }

        atomic_store(&reclaimer_idle, true, ATOMIC_SEQ_CST);
        if (atomic_load(&pending, ATOMIC_SEQ_CST))
            timer_arm(&poll_timer, clock_get_elapsed_ns() + EPOCH_POLL_NS);
        sched_block(&percpu_get()->sched);
    }
}

static void epoch_stage_init(struct boot_info*) {
    mutex_init(&reclaim_lock);
    timer_init(&poll_timer, poll_tick);

    struct task* task;
    errno_t status = task_create("epoch", process_kernel(), reclaimer_fn, 0, &task);
    assert(status == 0, "Unable to create the epoch reclaimer");
    sched_set_priority(task, SCHED_PRIO_WORK_HIGH);
    atomic_store(&reclaimer, task, ATOMIC_RELEASE);
    sched_add_task(&percpu_get()->sched, task);
}
INIT_TASK(epoch_stage, "lib.epoch", epoch_stage_init);
INIT_DEPENDS(epoch_stage, sched_stage);
