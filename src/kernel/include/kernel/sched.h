#pragma once

#include <kernel/init.h>
#include <kernel/list.h>
#include <kernel/mem.h>
#include <kernel/types.h>
#include <kernel/namespace.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>

ASSERT_TYPE(struct arch_context);
ASSERT_TYPE(struct arch_task_context);

enum task_state {
    TASK_STATE_RUNNING, // Task is active and running.
    TASK_STATE_READY,   // Task is active, but not currently scheduled.
    TASK_STATE_BLOCKED, // Task is waiting on another object.
};

// A task is the smallest unit of the scheduler.
struct task {
    size_t id;
    struct namespace* namespace;
    enum task_state state;
    struct arch_context context;
    virt_t kernel_stack;
    virt_t user_stack;
    size_t time_slice;
    int8_t priority;
};

// Per-CPU data for scheduling.
struct sched_percpu {
    struct task* current;
    struct task* idle_task;
    size_t preempt_level;
    SLIST_HEAD(struct task*) run_queue;
};

typedef void (*task_fn_t)(void* arg);

// Creates a new task.
zn_status_t task_create(task_fn_t entry, void* arg, struct task** out);

[[__init]]
void sched_init();

void sched_reschedule(struct sched_percpu* sched);

// Reschedules without adding the current task back to the run queue.
void sched_yield(struct sched_percpu* sched);

void sched_add_task(struct sched_percpu* sched, struct task* task);
