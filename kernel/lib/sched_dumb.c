// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/cpu.h>
#include <lib/irq.h>
#include <lib/list.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/sched_private.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <lib/turnstile.h>
#include <lib/utils.h>
#include <mem/pmap.h>
#include <mem/stack.h>
#include <mem/vma.h>
#include <stdint.h>

static void idle_func(uintptr_t) {
    while (1) {
        irq_set_state(true);
        irq_wait();
    }
}

constexpr int32_t SCHED_CPU_IDLE = -1;
constexpr int32_t SCHED_CPU_CLAIMED = -2;

static _Atomic(struct scheduler*) all_cpus;

static struct percpu* cpu_of(struct scheduler* sched) {
    return containerof(sched, struct percpu, sched);
}

static void preempt_tick(struct timer* timer) {
    struct scheduler* sched = containerof(timer, struct scheduler, preempt_timer);
    sched->need_resched = true;
}

static void start_time_slice(struct scheduler* sched, struct task* task, uint64_t now) {
    sched->need_resched = false;
    sched->slice_start = now;
    if (task == sched->idle_task)
        timer_cancel(&sched->preempt_timer);
    else
        timer_arm(&sched->preempt_timer, now + task->time_slice);
}

void sched_init(struct scheduler* sched) {
    spin_init(&sched->lock);
    atomic_store(&sched->kick_pending, false, ATOMIC_RELAXED);
    for (size_t i = 0; i < SCHED_PRIO_COUNT; i++)
        tailq_init(&sched->run_queue[i]);

    struct task* idle_task;
    assert(task_create("idle", process_kernel(), idle_func, 0, &idle_task) == 0, "Unable to create idle task!");
    idle_task->last_rq = sched;
    sched->idle_task = idle_task;

    struct task* boot_task;
    assert(task_create_current("boot", process_kernel(), &boot_task) == 0, "Unable to create boot task!");
    boot_task->state = TASK_STATE_RUNNING;
    boot_task->last_rq = sched;
    atomic_store(&boot_task->on_cpu, true, ATOMIC_RELAXED);
    sched->current = boot_task;
    atomic_store(&sched->current_priority, SCHED_PRIO_DEFAULT, ATOMIC_RELAXED);

    sched->next_cpu = atomic_load(&all_cpus, ATOMIC_RELAXED);
    while (!atomic_compare_exchange_weak(&all_cpus, &sched->next_cpu, sched, ATOMIC_RELEASE, ATOMIC_RELAXED))
        ;

    timer_cpu_init();
    timer_init(&sched->preempt_timer, preempt_tick);
    sched->boost_epoch = 0;
    start_time_slice(sched, boot_task, clock_get_elapsed_ns());

    log("sched: Scheduler initialized\n");
}

// Asks the CPU of `sched` to reschedule as soon as it has interrupts enabled.
static void kick(struct scheduler* sched) {
    if (sched == &percpu_get()->sched) {
        sched->owe_preempt = true;
        if (percpu_ref(irq_data)->depth == 0 && irq_get_state() && !sched->preempt_level && sched->current)
            sched_reschedule(sched);
        return;
    }
    if (!atomic_exchange(&sched->kick_pending, true, ATOMIC_RELAXED))
        arch_sched_kick(cpu_of(sched));
}

static void rq_insert(struct scheduler* sched, struct task* task) {
    uint8_t prio = atomic_load(&task->priority, ATOMIC_RELAXED);
    tailq_insert_tail(&sched->run_queue[prio], task, next);
    sched->ready_mask |= (uint64_t)1 << prio;
}

static void rq_remove(struct scheduler* sched, struct task* task) {
    uint8_t prio = atomic_load(&task->priority, ATOMIC_RELAXED);
    tailq_remove(&sched->run_queue[prio], task, next);
    if (tailq_empty(&sched->run_queue[prio]))
        sched->ready_mask &= ~((uint64_t)1 << prio);
}

// Returns -1 if no task is ready.
static int rq_top_priority(struct scheduler* sched) {
    return sched->ready_mask ? 63 - __builtin_clzll(sched->ready_mask) : -1;
}

static uint8_t effective_prio(struct task* task) {
    uint8_t prio = task->base_prio;
    if (prio < SCHED_PRIO_WORK_HIGH)
        prio -= MIN(task->penalty, prio);
    return MAX(prio, task->lent_prio);
}

static void refresh_prio(struct task* task) {
    if (task->blocked_ts == nullptr)
        atomic_store(&task->priority, effective_prio(task), ATOMIC_RELAXED);
}

static size_t quantum(struct task* task) {
    return SCHED_TIME_SLICE_NS * (task->penalty + 1);
}

static void feedback_reset(struct scheduler* sched, struct task* task) {
    task->penalty = 0;
    task->time_slice = quantum(task);
    task->feedback_epoch = sched->boost_epoch;
    refresh_prio(task);
}

static void boost(struct scheduler* sched) {
    for (uint8_t prio = 0; prio < SCHED_PRIO_WORK_HIGH; prio++) {
        struct task* t = tailq_first(&sched->run_queue[prio]);
        while (t) {
            struct task* after = tailq_next(t, next);
            if (t->penalty && t->feedback_epoch < sched->boost_epoch) {
                rq_remove(sched, t);
                feedback_reset(sched, t);
                rq_insert(sched, t);
            }
            t = after;
        }
    }
}

static void account(struct scheduler* sched, struct task* task, uint64_t now) {
    if (task->feedback_epoch < sched->boost_epoch)
        feedback_reset(sched, task);

    uint64_t ran = now - sched->slice_start;
    sched->slice_start = now;
    if (ran < task->time_slice) {
        task->time_slice -= ran;
        return;
    }

    if (task->base_prio < SCHED_PRIO_WORK_HIGH && task->penalty < SCHED_FEEDBACK_DEPTH)
        task->penalty++;
    task->time_slice = quantum(task);
    task->feedback_epoch = sched->boost_epoch;
    refresh_prio(task);
    sched->need_resched = true;
}

// Queues `task` on `sched` and preempts its CPU if `task` has a higher prio than what it's running.
static void enqueue(struct scheduler* sched, struct task* task) {
    spin_lock(&sched->lock);
    task->state = TASK_STATE_READY;
    task->last_rq = sched;
    if (task->feedback_epoch < sched->boost_epoch)
        feedback_reset(sched, task);
    refresh_prio(task);
    rq_insert(sched, task);
    bool preempt = atomic_load(&task->priority, ATOMIC_RELAXED) > atomic_load(&sched->current_priority, ATOMIC_RELAXED);
    spin_unlock(&sched->lock);

    if (preempt)
        kick(sched);
}

void sched_add_task(struct scheduler* sched, struct task* task) {
    assert(task != nullptr, "Tried to add NULL to the run queue");
    enqueue(sched, task);
}

static struct task* next_task(struct scheduler* sched, uint64_t now) {
    spin_lock(&sched->lock);
    uint64_t epoch = now / SCHED_BOOST_INTERVAL_NS;
    if (epoch > sched->boost_epoch) {
        sched->boost_epoch = epoch;
        boost(sched);
    }

    int prio = rq_top_priority(sched);
    if (prio < 0) {
        atomic_store(&sched->current_priority, SCHED_CPU_IDLE, ATOMIC_RELAXED);
        spin_unlock(&sched->lock);
        return sched->idle_task;
    }

    struct task* t = tailq_first(&sched->run_queue[prio]);
    rq_remove(sched, t);
    t->state = TASK_STATE_RUNNING;
    atomic_store(&sched->current_priority, prio, ATOMIC_RELAXED);
    spin_unlock(&sched->lock);

    return t;
}

static void finish_switch(struct task* prev) {
    // The context of `prev` is fully saved now, so other CPUs can switch to it.
    atomic_store(&prev->on_cpu, false, ATOMIC_RELEASE);
    if (prev->state == TASK_STATE_DEAD)
        task_reap(prev);
}

static void do_reschedule(struct scheduler* sched, uint64_t now) {
    atomic_store(&sched->kick_pending, false, ATOMIC_RELAXED);
    sched->owe_preempt = false;
    struct task* from = sched->current;
    struct task* to = next_task(sched, now);

    if (from != to || sched->need_resched)
        start_time_slice(sched, to, now);

    // We'd end up on the same task, skip switching altogether.
    if (from == to) {
        irq_unlock();
        return;
    }

    // A task that was woken by another CPU might not have finished switching away on its old CPU yet.
    while (atomic_load(&to->on_cpu, ATOMIC_ACQUIRE))
        cpu_pause();
    atomic_store(&to->on_cpu, true, ATOMIC_RELAXED);

    sched->current = to;
    pmap_set(&to->space->pmap);

    struct percpu* cpu = percpu_get();
    from->user_stack = cpu->user_stack;
    cpu->kernel_stack = kernel_stack_top(&to->kernel_stack);
    cpu->user_stack = to->user_stack;

    struct irq_percpu irq_state = *percpu_ref(irq_data);
    struct task* prev = arch_sched_switch(from, to);
    *percpu_ref(irq_data) = irq_state;
    finish_switch(prev);
    irq_unlock();
}

void sched_reschedule(struct scheduler* sched) {
    irq_lock();
    struct task* current = sched->current;
    uint64_t now = clock_get_elapsed_ns();
    if (current != sched->idle_task && current->state == TASK_STATE_RUNNING) {
        spin_lock(&sched->lock);
        account(sched, current, now);
        current->state = TASK_STATE_READY;
        rq_insert(sched, current);
        spin_unlock(&sched->lock);
    }
    do_reschedule(sched, now);
}

void sched_yield(struct scheduler* sched) {
    irq_lock();
    do_reschedule(sched, clock_get_elapsed_ns());
}

void sched_exit() {
    irq_lock();
    struct scheduler* sched = &percpu_get()->sched;
    sched->current->state = TASK_STATE_DEAD;
    do_reschedule(sched, clock_get_elapsed_ns());
    unreachable();
}

void sched_block(struct scheduler* sched) {
    irq_lock();
    struct task* current = sched->current;

    task_lock(current);
    if (atomic_exchange(&current->wake_pending, false, ATOMIC_ACQ_REL)) {
        task_unlock(current);
        irq_unlock();
        return;
    }

    uint64_t now = clock_get_elapsed_ns();
    spin_lock(&sched->lock);
    account(sched, current, now);
    spin_unlock(&sched->lock);

    current->state = TASK_STATE_BLOCKED;
    task_unlock(current);

    do_reschedule(sched, now);
}

static struct scheduler* select_cpu(struct task* task) {
    struct scheduler* last = task->last_rq ? task->last_rq : &percpu_get()->sched;
    int prio = atomic_load(&task->priority, ATOMIC_RELAXED);
    if (atomic_load(&last->current_priority, ATOMIC_RELAXED) < prio)
        return last;

    // Claim an idle CPU.
    struct scheduler* s;
    for (s = atomic_load(&all_cpus, ATOMIC_ACQUIRE); s; s = s->next_cpu) {
        int idle = SCHED_CPU_IDLE;
        if (atomic_compare_exchange(&s->current_priority, &idle, SCHED_CPU_CLAIMED, ATOMIC_RELAXED, ATOMIC_RELAXED))
            return s;
    }
    return last;
}

void sched_wake_locked(struct task* task) {
    if (task->state == TASK_STATE_BLOCKED)
        enqueue(select_cpu(task), task);
    else
        atomic_store(&task->wake_pending, true, ATOMIC_RELEASE);
}

void sched_wake(struct task* task) {
    task_lock(task);
    sched_wake_locked(task);
    task_unlock(task);
}

void sched_notify(struct task* task) {
    task_lock(task);
    if (task->state == TASK_STATE_BLOCKED) {
        enqueue(select_cpu(task), task);
    } else {
        atomic_store(&task->wake_pending, true, ATOMIC_RELEASE);
        if (task->state == TASK_STATE_RUNNING && task->last_rq && task->last_rq != &percpu_get()->sched)
            kick(task->last_rq);
    }
    task_unlock(task);
}

bool sched_update_priority_locked(struct task* task) {
    struct scheduler* sched = task->last_rq;
    if (sched == nullptr) {
        // The task was never scheduled, so it isn't queued anywhere.
        uint8_t new_prio = effective_prio(task);
        return atomic_exchange(&task->priority, new_prio, ATOMIC_RELAXED) != new_prio;
    }

    bool preempt = false;
    spin_lock(&sched->lock);
    uint8_t new_prio = effective_prio(task);
    if (new_prio == atomic_load(&task->priority, ATOMIC_RELAXED)) {
        spin_unlock(&sched->lock);
        return false;
    }

    if (task->state == TASK_STATE_READY) {
        rq_remove(sched, task);
        atomic_store(&task->priority, new_prio, ATOMIC_RELAXED);
        rq_insert(sched, task);
        preempt = new_prio > atomic_load(&sched->current_priority, ATOMIC_RELAXED);
    } else {
        atomic_store(&task->priority, new_prio, ATOMIC_RELAXED);
        if (task->state == TASK_STATE_RUNNING) {
            atomic_store(&sched->current_priority, new_prio, ATOMIC_RELAXED);
            preempt = rq_top_priority(sched) > new_prio;
        }
    }
    spin_unlock(&sched->lock);

    if (preempt)
        kick(sched);
    return true;
}

void sched_set_priority(struct task* task, uint8_t priority) {
    assert(priority <= SCHED_PRIO_MAX, "Priority %u is out of range", priority);
    task_lock(task);
    task->base_prio = priority;
    if (sched_update_priority_locked(task))
        turnstile_reprioritize(task);
    else
        task_unlock(task);
}

void task_entry(task_fn_t entry, uintptr_t arg0, struct task* prev) {
    finish_switch(prev);

    *percpu_ref(irq_data) = (struct irq_percpu){
        .depth = 1,
        .saved_state = true,
    };
    irq_unlock();

    entry(arg0);
    sched_exit();
}

[[__init]]
static void sched_stage_init(struct boot_info*) {
    sched_init(&percpu_get()->sched);
}
INIT_TASK(sched_stage, "lib.sched", sched_stage_init);
INIT_DEPENDS(sched_stage, percpu_stage);
