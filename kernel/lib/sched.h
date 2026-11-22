// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/init.h>
#include <lib/initgraph.h>
#include <lib/signal.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <lib/timer.h>
#include <lib/types.h>
#include <mem/stack.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>

ASSERT_TYPE(arch_context_t);
ASSERT_TYPE(arch_task_context_t);

enum task_state {
    TASK_STATE_RUNNING, // Task is active and running.
    TASK_STATE_READY,   // Task is active, but not currently scheduled.
    TASK_STATE_BLOCKED, // Task is waiting on another object.
    TASK_STATE_DEAD,    // Task is killed.
};

enum sched_prio : uint8_t {
    SCHED_PRIO_MIN = 0,

    SCHED_PRIO_DEFAULT = 32,
    SCHED_PRIO_WORK_HIGH = 48,
    SCHED_PRIO_IRQ = 56,

    SCHED_PRIO_MAX = 63,
    SCHED_PRIO_COUNT = SCHED_PRIO_MAX + 1,
};

constexpr size_t SCHED_TIME_SLICE_NS = 10'000'000;
constexpr uint8_t SCHED_FEEDBACK_DEPTH = 8;
constexpr uint64_t SCHED_BOOST_INTERVAL_NS = 1'000'000'000;

struct process;
struct turnstile;

constexpr size_t TASK_NAME_MAX = 128;

// A task is the smallest unit of the scheduler.
struct task {
    size_t id;
    char name[TASK_NAME_MAX];
    struct vma* space;
    struct process* process;
    tailq_link(struct task) process_link;
    enum task_state state;
    arch_task_context_t context;
    struct usercopy_region* usercopy_region;
    struct stack kernel_stack;
    uintptr_t user_stack; // Saved stack pointer from userspace.
    size_t time_slice;
    uint8_t penalty;
    uint64_t feedback_epoch;
    atomic_bool wake_pending;

    struct spinlock lock;
    _Atomic(struct spinlock*) lockp;

    atomic_bool on_cpu;                // Whether a CPU is still running on this task's context.
    uint8_t base_prio;                 // Set by sched_set_priority().
    uint8_t lent_prio;                 // Highest priority lent by tasks waiting on locks this task owns.
    _Atomic(enum sched_prio) priority; // The effective priority of this task.
    struct scheduler* last_rq;         // The most recent run queue this task was scheduled on.

    struct turnstile* turnstile;             // Turnstile owned by this task.
    struct turnstile* blocked_ts;            // The turnstile this task is blocked on.
    tailq_head(struct turnstile) pi_lenders; // Turnstiles of locks this task owns.
    tailq_link(struct task) ts_link;         // Link in the waiters of `blocked_ts`.
    uint8_t ts_queue;
    atomic_bool ts_woken;

    struct task_signals signals;

    tailq_link(struct task) next;
};

// Per-CPU data for scheduling.
struct scheduler {
    struct task* current;
    struct task* idle_task;
    size_t preempt_level;

    struct spinlock lock;

    // Bit N is set if `run_queue[N]` is not empty.
    uint64_t ready_mask;
    tailq_head(struct task) run_queue[SCHED_PRIO_COUNT];

    // Current priority, or negative if the CPU is idle.
    _Atomic(int32_t) current_priority;

    atomic_bool kick_pending;

    struct scheduler* next_cpu;

    struct timer preempt_timer;
    bool need_resched;
    bool owe_preempt;
    uint64_t slice_start;
    uint64_t boost_epoch;
};

typedef void (*task_fn_t)(uintptr_t arg0);

errno_t task_create(const char* name, struct process* proc, task_fn_t entry, uintptr_t arg0, struct task** out);
void task_destroy(struct task* task);
void task_reap(struct task* task);
void task_lock(struct task* task);
void task_unlock(struct task* task);
[[noreturn]]
void task_entry(task_fn_t entry, uintptr_t arg0, struct task* prev);

void sched_init(struct scheduler* sched);
void sched_reschedule(struct scheduler* sched);
void sched_yield(struct scheduler* sched);
void sched_add_task(struct scheduler* sched, struct task* task);

// Blocks the current task until sched_wake() is called on it.
// Returns immediately if a wake is already pending. Must not hold any spinlock.
void sched_block(struct scheduler* sched);

// Makes a blocked task runnable again, or marks the wake as pending if it hasn't blocked yet.
void sched_wake(struct task* task);

// Same as sched_wake(), but the task must be locked with task_lock().
void sched_wake_locked(struct task* task);

void sched_notify(struct task* task);

void sched_sleep_until(uint64_t deadline_ns);

void sched_preempt_disable();
void sched_preempt_enable();

[[noreturn]]
void sched_exit();

// Sets the base priority of `task`.
void sched_set_priority(struct task* task, uint8_t priority);

[[noreturn]]
void sched_to_user(uintptr_t ctx);
[[noreturn]]
void sched_to_user_context(uintptr_t ctx);

// Info block for sched_to_user.
struct task_start_info {
    uintptr_t ip;
    uintptr_t sp;
    uintptr_t arg;
};

INIT_EXTERN(sched_stage);
