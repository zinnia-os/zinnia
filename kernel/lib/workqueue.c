// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/spin.h>
#include <lib/workqueue.h>
#include <mem/alloc.h>

struct workqueue {
    struct spinlock lock;
    tailq_head(struct work) queue;
    struct task* worker;
    bool idle;
    bool stopping;
    struct task* exit_waiter;
    atomic_bool exited;
};

struct work_barrier {
    struct work work;
    struct task* waiter;
    atomic_bool done;
};

struct workqueue* workqueue_system;
struct workqueue* workqueue_high;

void work_init(struct work* work, void (*func)(struct work* work)) {
    work->func = func;
    atomic_init(&work->pending, false);
}

static struct task* current_task() {
    return percpu_get()->sched.current;
}

static void wait_for(atomic_bool* flag) {
    while (!atomic_load(flag, ATOMIC_ACQUIRE))
        sched_block(&percpu_get()->sched);
}

static void signal(atomic_bool* flag, struct task* waiter) {
    atomic_store(flag, true, ATOMIC_RELEASE);
    sched_wake(waiter);
}

static struct work* next_work(struct workqueue* wq, void (**out_func)(struct work* work)) {
    spin_lock(&wq->lock);
    while (true) {
        struct work* work = tailq_first(&wq->queue);
        if (work) {
            tailq_remove(&wq->queue, work, link);
            *out_func = work->func;
            atomic_store(&work->pending, false, ATOMIC_RELEASE);
            spin_unlock(&wq->lock);
            return work;
        }
        if (wq->stopping) {
            spin_unlock(&wq->lock);
            return nullptr;
        }

        wq->idle = true;
        spin_unlock(&wq->lock);
        sched_block(&percpu_get()->sched);
        spin_lock(&wq->lock);
    }
}

// Worker task main loop.
static void worker_fn(uintptr_t arg) {
    struct workqueue* wq = (struct workqueue*)arg;

    struct work* work;
    void (*func)(struct work* work);
    while ((work = next_work(wq, &func)))
        func(work);

    struct task* waiter = wq->exit_waiter;
    signal(&wq->exited, waiter);
    sched_exit();
}

errno_t workqueue_create(const char* name, uint8_t priority, struct workqueue** out) {
    struct workqueue* wq = mem_alloc(sizeof(*wq), 0);
    if (!wq)
        return ENOMEM;

    spin_init(&wq->lock);
    tailq_init(&wq->queue);
    atomic_init(&wq->exited, false);

    errno_t status = task_create(name, process_kernel(), worker_fn, (uintptr_t)wq, &wq->worker);
    if (status) {
        mem_free(wq, sizeof(*wq));
        return status;
    }

    sched_set_priority(wq->worker, priority);
    sched_add_task(&percpu_get()->sched, wq->worker);

    *out = wq;
    return 0;
}

static void wake_worker(struct workqueue* wq, bool stop) {
    spin_lock(&wq->lock);
    if (stop)
        wq->stopping = true;
    const bool was_idle = wq->idle;
    wq->idle = false;
    spin_unlock(&wq->lock);

    if (was_idle)
        sched_wake(wq->worker);
}

void workqueue_destroy(struct workqueue* wq) {
    wq->exit_waiter = current_task();
    wake_worker(wq, true);
    wait_for(&wq->exited);
    mem_free(wq, sizeof(*wq));
}

bool work_queue(struct workqueue* wq, struct work* work) {
    if (atomic_exchange(&work->pending, true, ATOMIC_ACQ_REL))
        return false;

    spin_lock(&wq->lock);
    assert(!wq->stopping, "Queued work on a workqueue that is being destroyed");
    tailq_insert_tail(&wq->queue, work, link);
    const bool was_idle = wq->idle;
    wq->idle = false;
    spin_unlock(&wq->lock);

    if (was_idle)
        sched_wake(wq->worker);
    return true;
}

bool work_cancel(struct workqueue* wq, struct work* work) {
    bool removed = false;
    spin_lock(&wq->lock);
    for (struct work* it = tailq_first(&wq->queue); it; it = tailq_next(it, link)) {
        if (it == work) {
            tailq_remove(&wq->queue, work, link);
            atomic_store(&work->pending, false, ATOMIC_RELEASE);
            removed = true;
            break;
        }
    }
    spin_unlock(&wq->lock);
    return removed;
}

static void barrier_fn(struct work* work) {
    struct work_barrier* barrier = containerof(work, struct work_barrier, work);
    struct task* waiter = barrier->waiter;
    signal(&barrier->done, waiter);
}

void workqueue_flush(struct workqueue* wq) {
    assert(current_task() != wq->worker, "Tried to flush a workqueue from its own worker");

    struct work_barrier barrier = {.waiter = current_task()};
    work_init(&barrier.work, barrier_fn);
    atomic_init(&barrier.done, false);
    work_queue(wq, &barrier.work);
    wait_for(&barrier.done);
}

static void workqueue_stage_init(struct boot_info* info) {
    errno_t status = workqueue_create("kworker", SCHED_PRIO_DEFAULT, &workqueue_system);
    assert(status == 0, "Unable to create a workqueue");

    status = workqueue_create("kworker/high", SCHED_PRIO_WORK_HIGH, &workqueue_high);
    assert(status == 0, "Unable to create a workqueue");
}

INIT_TASK(workqueue_stage, "lib.workqueue", workqueue_stage_init);
INIT_DEPENDS(workqueue_stage, sched_stage);
