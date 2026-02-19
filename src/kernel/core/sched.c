#include <zinnia/status.h>
#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/irq.h>
#include <kernel/list.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/tailq.h>
#include <bits/irq.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>

static void idle_func(uintptr_t, uintptr_t) {
    while (1) {
        arch_irq_set_state(true);
        arch_irq_wait();
    }
}

void sched_to_user(uintptr_t ip, uintptr_t sp) {
    arch_sched_jump_to_user(ip, sp);
}

void sched_to_user_context(uintptr_t ctx, uintptr_t) {
    arch_sched_jump_to_context((struct arch_context*)ctx);
}

void sched_init(struct sched_percpu* sched) {
    struct task* idle_task;
    ASSERT(
        task_create("idle", &kernel_vas, nullptr, idle_func, 0, 0, &idle_task) == ZN_OK,
        "Unable to create idle task!\n"
    );
    sched->idle_task = idle_task;
    sched->current = idle_task;

    TAILQ_INIT(&sched->run_queue);

    kprintf("Scheduler initialized\n");
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

    pt_set(&to->space->pt);

    struct percpu* cpu = percpu_get();
    from->kernel_stack = cpu->kernel_stack;
    from->user_stack = cpu->user_stack;
    cpu->kernel_stack = to->kernel_stack;
    cpu->user_stack = to->user_stack;

    arch_sched_switch(from, to);
}

void sched_reschedule(struct sched_percpu* sched) {
    irq_lock();
    sched_add_task(sched, sched->current);
    do_reschedule(sched);
}

void sched_yield(struct sched_percpu* sched) {
    irq_lock();
    do_reschedule(sched);
}

zn_status_t task_create(
    const char* name,
    struct vas* space,
    struct namespace* ns,
    task_fn_t entry,
    uintptr_t arg0,
    uintptr_t arg1,
    struct task** out
) {
    if (!out)
        return ZN_ERR_INVALID;

    struct task* new_task = mem_alloc(sizeof(struct task), 0);
    if (new_task == nullptr)
        return ZN_ERR_NO_MEMORY;

    static size_t last_tid = 0;
    new_task->id = atomic_fetch_add(&last_tid, 1);
    new_task->space = space;
    new_task->namespace = ns;
    strncpy(new_task->name, name, sizeof(new_task->name));

    // Allocate a kernel stack.
    new_task->kernel_stack = (uintptr_t)mem_alloc(KERNEL_STACK_SIZE, 0);
    if (!new_task->kernel_stack)
        return ZN_ERR_NO_MEMORY;
    new_task->user_stack = 0;

    // Initialize the arch-dependent parts.
    zn_status_t s =
        arch_task_init(&new_task->context, entry, arg0, arg1, new_task->kernel_stack + KERNEL_STACK_SIZE, true);
    if (s != ZN_OK)
        return s;

    *out = new_task;
    return ZN_OK;
}

void task_entry(task_fn_t entry, uintptr_t arg0, uintptr_t arg1) {
    entry(arg0, arg1);

    struct sched_percpu* sched = &percpu_get()->sched;
    sched->current->state = TASK_STATE_DEAD;
    sched_yield(sched);
    panic("unreachable");
}
