// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/irq.h>
#include <lib/park.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/sched_private.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <lib/turnstile.h>
#include <mem/alloc.h>
#include <mem/stack.h>
#include <mem/vma.h>
#include <bits/preempt.h>
#include <stdint.h>
#include <string.h>

void sched_to_user(uintptr_t info_ptr) {
    struct task_start_info* info = (struct task_start_info*)info_ptr;
    uintptr_t ip = info->ip;
    uintptr_t sp = info->sp;
    uintptr_t arg = info->arg;
    mem_free(info, sizeof(*info));
    arch_sched_jump_to_user(ip, sp, arg);
}

void sched_to_user_context(uintptr_t ctx) {
    arch_context_t context = *(arch_context_t*)ctx;
    mem_free((void*)ctx, sizeof(arch_context_t));
    arch_sched_jump_to_context(&context);
}

static struct spinlock task_cache_lock;
static tailq_head(struct task) task_cache = {nullptr, &task_cache.tqh_first};

static struct task* task_alloc() {
    spin_lock(&task_cache_lock);
    struct task* task = tailq_first(&task_cache);
    if (task)
        tailq_remove(&task_cache, task, next);
    spin_unlock(&task_cache_lock);

    if (task == nullptr)
        return mem_alloc(sizeof(struct task), 0);

    size_t keep_start = offsetof(struct task, on_cpu);
    size_t keep_end = keep_start + sizeof(task->on_cpu);
    memset(task, 0, keep_start);
    memset((char*)task + keep_end, 0, sizeof(struct task) - keep_end);
    return task;
}

static void task_release(struct task* task) {
    spin_lock(&task_cache_lock);
    tailq_insert_tail(&task_cache, task, next);
    spin_unlock(&task_cache_lock);
}

static void task_init_common(struct task* task, const char* name, struct process* proc) {
    task->process = proc;
    task->space = proc->space;
    process_ref(proc);
    if (task->space != &kernel_vma)
        vma_ref(task->space);
    spin_init(&task->lock);
    atomic_store(&task->lockp, &task->lock, ATOMIC_RELAXED);
    strncpy(task->name, name, sizeof(task->name));
    task->base_prio = process_priority(proc);
    task->time_slice = SCHED_TIME_SLICE_NS;
    task->penalty = 0;
    task->feedback_epoch = 0;
    atomic_store(&task->priority, task->base_prio, ATOMIC_RELAXED);
    tailq_init(&task->pi_lenders);
    process_attach_task(proc, task);
}

errno_t task_create_current(const char* name, struct process* proc, struct task** out) {
    struct task* task = task_alloc();
    if (task == nullptr)
        return ENOMEM;

    task->turnstile = turnstile_alloc();
    if (task->turnstile == nullptr) {
        task_release(task);
        return ENOMEM;
    }

    task_init_common(task, name, proc);
    *out = task;
    return 0;
}

constexpr size_t KERNEL_STACK_SIZE = 16 * 1024;

errno_t task_create(const char* name, struct process* proc, task_fn_t entry, uintptr_t arg0, struct task** out) {
    if (!out)
        return EINVAL;

    struct task* new_task = task_alloc();
    if (new_task == nullptr)
        return ENOMEM;

    static atomic_size_t last_tid = 0;
    new_task->id = atomic_fetch_add(&last_tid, 1, ATOMIC_ACQUIRE);
    new_task->turnstile = turnstile_alloc();
    if (new_task->turnstile == nullptr) {
        task_release(new_task);
        return ENOMEM;
    }

    // Allocate a kernel stack.
    errno_t s = stack_init(&new_task->kernel_stack, KERNEL_STACK_SIZE);
    if (s != 0) {
        turnstile_free(new_task->turnstile);
        task_release(new_task);
        return s;
    }
    new_task->user_stack = 0;

    // Initialize the arch-dependent parts.
    s = arch_task_init(&new_task->context, entry, arg0, kernel_stack_top(&new_task->kernel_stack), true);
    if (s != 0) {
        stack_destroy(&new_task->kernel_stack);
        turnstile_free(new_task->turnstile);
        task_release(new_task);
        return s;
    }

    task_init_common(new_task, name, proc);
    *out = new_task;
    return 0;
}

void task_destroy(struct task* task) {
    assert(tailq_empty(&task->pi_lenders), "Task %zu exited while owning contended locks", task->id);
    turnstile_free(task->turnstile);
    arch_task_fini(&task->context);
    stack_destroy(&task->kernel_stack);
    process_detach_task(task->process, task);
    if (task->space != &kernel_vma)
        vma_unref(task->space);
    process_unref(task->process);
    task_release(task);
}

static struct spinlock task_reap_lock;
static tailq_head(struct task) task_reap_list = {nullptr, &task_reap_list.tqh_first};
static _Atomic(struct task*) task_reaper;

void task_reap(struct task* task) {
    spin_lock(&task_reap_lock);
    tailq_insert_tail(&task_reap_list, task, next);
    spin_unlock(&task_reap_lock);

    struct task* reaper = atomic_load(&task_reaper, ATOMIC_ACQUIRE);
    if (reaper)
        sched_wake(reaper);
}

static void task_reaper_fn(uintptr_t) {
    while (true) {
        spin_lock(&task_reap_lock);
        struct task* task = tailq_first(&task_reap_list);
        if (task)
            tailq_remove(&task_reap_list, task, next);
        spin_unlock(&task_reap_lock);

        if (task)
            task_destroy(task);
        else
            sched_block(&percpu_get()->sched);
    }
}

static void task_reaper_stage_init(struct boot_info*) {
    struct task* task;
    errno_t status = task_create("reaper", process_kernel(), task_reaper_fn, 0, &task);
    assert(status == 0, "Unable to create task reaper");
    sched_set_priority(task, SCHED_PRIO_WORK_HIGH);
    atomic_store(&task_reaper, task, ATOMIC_RELEASE);
    sched_add_task(&percpu_get()->sched, task);
}
INIT_TASK(task_reaper_stage, "lib.task_reaper", task_reaper_stage_init);
INIT_DEPENDS(task_reaper_stage, sched_stage);

void task_lock(struct task* task) {
    while (true) {
        struct spinlock* lock = atomic_load(&task->lockp, ATOMIC_ACQUIRE);
        spin_lock(lock);
        if (atomic_load(&task->lockp, ATOMIC_RELAXED) == lock)
            return;
        spin_unlock(lock);
    }
}

void task_unlock(struct task* task) {
    spin_unlock(atomic_load(&task->lockp, ATOMIC_RELAXED));
}

void sched_sleep_until(uint64_t deadline_ns) {
    const uint8_t key = 0;
    park_timeout(&key, nullptr, nullptr, nullptr, 0, &deadline_ns);
}

void sched_preempt_disable() {
    arch_sched_preempt_disable();
}

void sched_preempt_enable() {
    if (!arch_sched_preempt_enable())
        return;
    struct scheduler* sched = &percpu_get()->sched;
    if ((sched->owe_preempt || sched->need_resched) && percpu_ref(irq_data)->depth == 0 && irq_get_state() &&
        sched->current)
        sched_reschedule(sched);
}
