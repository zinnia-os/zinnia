#pragma once

#include <kernel/alloc.h>
#include <kernel/init.h>
#include <kernel/tailq.h>
#include <kernel/types.h>
#include <kernel/wait.h>
#include <uapi/errno.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>

ASSERT_TYPE(struct arch_context);
ASSERT_TYPE(struct arch_task_context);

enum task_state {
    TASK_STATE_RUNNING, // Task is active and running.
    TASK_STATE_READY,   // Task is active, but not currently scheduled.
    TASK_STATE_BLOCKED, // Task is waiting on another object.
    TASK_STATE_DEAD,    // Task is killed.
};

#define TASK_NAME_MAX     128
#define KERNEL_STACK_SIZE 0x4000

// A task is the smallest unit of the scheduler.
struct task {
    size_t id;
    char name[TASK_NAME_MAX];
    struct vm_space* space;
    enum task_state state;
    struct arch_task_context context;
    uintptr_t kernel_stack;
    uintptr_t user_stack;
    size_t time_slice;
    int8_t priority;

    TAILQ_LINK(struct task) next;
};

// Per-CPU data for scheduling.
struct sched_percpu {
    struct task* current;
    struct task* idle_task;
    size_t preempt_level;
    TAILQ_HEAD(struct task) run_queue;
};

// Blocks the current task on the given wait queue.
// The caller must NOT hold any spinlocks. Returns after the task is woken.
void sched_block(struct sched_percpu* sched, struct wait_queue* wq);

// Wakes the first task blocked on the given wait queue, if any.
void sched_wake(struct sched_percpu* sched, struct wait_queue* wq);

// Wakes all tasks blocked on the given wait queue.
void sched_wake_all(struct sched_percpu* sched, struct wait_queue* wq);

typedef void (*task_fn_t)(uintptr_t arg0);

errno_t task_create(const char* name, struct vm_space* space, task_fn_t entry, uintptr_t arg0, struct task** out);
[[noreturn]]
void task_entry(task_fn_t entry, uintptr_t arg0);

void sched_init(struct sched_percpu* sched);
void sched_reschedule(struct sched_percpu* sched);
void sched_yield(struct sched_percpu* sched);
void sched_add_task(struct sched_percpu* sched, struct task* task);
[[noreturn]]
void sched_to_user(uintptr_t ctx);
[[noreturn]]
void sched_to_user_context(uintptr_t ctx);

// Info block for sched_to_user. Heap-allocated by the caller, freed by sched_to_user_arg.
struct task_start_info {
    uintptr_t ip;
    uintptr_t sp;
    uintptr_t arg;
};

void arch_sched_preempt_disable();
bool arch_sched_preempt_enable();
void arch_sched_switch(struct task* from, struct task* to);
errno_t arch_task_init(
    struct arch_task_context* context,
    void* entry,
    uintptr_t arg0,
    uintptr_t stack_start,
    bool is_user
);
[[noreturn]]
void arch_sched_jump_to_context(struct arch_context* context);
[[noreturn]]
void arch_sched_jump_to_user(uintptr_t ip, uintptr_t sp, uintptr_t arg);
