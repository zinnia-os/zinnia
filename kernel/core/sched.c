#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/irq.h>
#include <kernel/list.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/process.h>
#include <kernel/sched.h>
#include <kernel/spin.h>
#include <kernel/tailq.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>
#include <bits/irq.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>

static void idle_func(uintptr_t) {
    while (1) {
        arch_irq_set_state(true);
        arch_irq_wait();
    }
}

static void dummy(uintptr_t) {
    ASSERT(false, "Attempted to run the dummy task!\n");
}

void sched_to_user(uintptr_t info_ptr) {
    struct task_start_info* info = (struct task_start_info*)info_ptr;
    uintptr_t ip = info->ip;
    uintptr_t sp = info->sp;
    uintptr_t arg = info->arg;
    mem_free(info);
    arch_sched_jump_to_user(ip, sp, arg);
}

void sched_to_user_context(uintptr_t ctx) {
    arch_sched_jump_to_context((struct arch_context*)ctx);
}

void sched_init(struct sched_percpu* sched) {
    ASSERT(process_new(nullptr, &kernel_space, &kernel_process) == 0, "Failed to create kernel process!\n");

    struct task* idle_task;
    ASSERT(task_create("idle", kernel_process, idle_func, 0, &idle_task) == 0, "Unable to create idle task!\n");
    sched->idle_task = idle_task;

    struct task* bootstrap_task;
    ASSERT(task_create("dummy", kernel_process, dummy, 0, &bootstrap_task) == 0, "Unable to create dummy task!\n");
    sched->current = bootstrap_task;

    TAILQ_INIT(&sched->run_queue);

    kprintf("sched: Scheduler initialized\n");
}

void sched_add_task(struct sched_percpu* sched, struct task* task) {
    ASSERT(task != nullptr, "Tried to add NULL to the run queue\n");
    TAILQ_INSERT_TAIL(&sched->run_queue, task, next);
}

static struct task* next_task(struct sched_percpu* sched) {
    struct task* t = TAILQ_FIRST(&sched->run_queue);

    if (t == nullptr)
        return sched->idle_task;

    TAILQ_REMOVE(&sched->run_queue, t, next);

    return t;
}

static void do_reschedule(struct sched_percpu* sched) {
    struct task* from = atomic_load_explicit(&sched->current, memory_order_acquire);
    struct task* to = next_task(sched);

    if (from == to) {
        irq_unlock();
        return;
    }

    atomic_store_explicit(&sched->current, to, memory_order_relaxed);

    pmap_set(&to->parent->address_space->pmap);

    struct percpu* cpu = percpu_get();
    from->kernel_stack = cpu->kernel_stack;
    from->user_stack = cpu->user_stack;
    cpu->kernel_stack = to->kernel_stack;
    cpu->user_stack = to->user_stack;

    arch_sched_switch(from, to);
}

void sched_reschedule(struct sched_percpu* sched) {
    irq_lock();
    if (sched->current->state != TASK_STATE_DEAD)
        sched_add_task(sched, sched->current);
    do_reschedule(sched);
}

void sched_yield(struct sched_percpu* sched) {
    irq_lock();
    do_reschedule(sched);
}

errno_t task_create(const char* name, struct process* parent, task_fn_t entry, uintptr_t arg0, struct task** out) {
    if (!out)
        return EINVAL;

    struct task* new_task = mem_alloc(sizeof(struct task), 0);
    if (new_task == nullptr)
        return ENOMEM;

    static size_t last_tid = 0;
    new_task->id = atomic_fetch_add(&last_tid, 1);
    new_task->parent = parent;
    strncpy(new_task->name, name, sizeof(new_task->name));

    // Allocate a kernel stack.
    new_task->kernel_stack = (uintptr_t)mem_alloc(KERNEL_STACK_SIZE, 0);
    if (!new_task->kernel_stack)
        return ENOMEM;
    new_task->user_stack = 0;

    // Initialize the arch-dependent parts.
    errno_t s = arch_task_init(&new_task->context, entry, arg0, new_task->kernel_stack + KERNEL_STACK_SIZE, true);
    if (s != 0)
        return s;

    *out = new_task;
    return 0;
}

void task_entry(task_fn_t entry, uintptr_t arg0) {
    entry(arg0);

    struct sched_percpu* sched = &percpu_get()->sched;
    sched->current->state = TASK_STATE_DEAD;
    sched_yield(sched);
    panic("unreachable");
}

void wait_queue_init(struct wait_queue* wq) {
    TAILQ_INIT(&wq->waiters);
    wq->lock = (struct spinlock){0};
}

void sched_block(struct sched_percpu* sched, struct wait_queue* wq) {
    irq_lock();
    spin_lock(&wq->lock);

    sched->current->state = TASK_STATE_BLOCKED;
    TAILQ_INSERT_TAIL(&wq->waiters, sched->current, next);

    spin_unlock(&wq->lock);
    do_reschedule(sched);
}

void sched_wake(struct sched_percpu* sched, struct wait_queue* wq) {
    spin_lock(&wq->lock);

    struct task* t = TAILQ_FIRST(&wq->waiters);
    if (t != nullptr) {
        TAILQ_REMOVE(&wq->waiters, t, next);
        t->state = TASK_STATE_READY;
        sched_add_task(sched, t);
    }

    spin_unlock(&wq->lock);
}

void sched_wake_all(struct sched_percpu* sched, struct wait_queue* wq) {
    spin_lock(&wq->lock);

    struct task* t;
    while ((t = TAILQ_FIRST(&wq->waiters)) != nullptr) {
        TAILQ_REMOVE(&wq->waiters, t, next);
        t->state = TASK_STATE_READY;
        sched_add_task(sched, t);
    }

    spin_unlock(&wq->lock);
}
