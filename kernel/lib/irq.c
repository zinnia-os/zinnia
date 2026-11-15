// SPDX-License-Identifier: BSD-3-Clause

#include <arch/cpu.h>
#include <arch/irq.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/irq.h>
#include <lib/list.h>
#include <lib/mutex.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/alloc.h>

[[__percpu]]
struct irq_percpu irq_data;

bool irq_set_state(bool state) {
    return arch_irq_set_state(state);
}

bool irq_get_state() {
    return arch_irq_get_state();
}

void irq_wait() {
    arch_irq_wait();
}

void irq_lock() {
    bool prev = arch_irq_set_state(false);
    struct irq_percpu* state = percpu_ref(irq_data);
    if (state->depth++ == 0)
        state->saved_state = prev;
}

void irq_unlock() {
    struct irq_percpu* state = percpu_ref(irq_data);
    if (--state->depth == 0 && state->saved_state) {
        struct scheduler* sched = &percpu_get()->sched;
        if (sched->owe_preempt && !sched->preempt_level && sched->current)
            sched_reschedule(sched);
        arch_irq_set_state(true);
    }
}

void irq_line_program(struct irq_line* line, enum irq_trigger_mode trigger, enum irq_polarity polarity) {
    irq_lock();
    line->mode = line->set_config(line, trigger, polarity);
    line->trigger = trigger;
    irq_unlock();
}

struct irq_task {
    struct irq_line* line;
    struct task* task;
    _Atomic(bool) need;
    _Atomic(bool) masked;
    _Atomic(bool) running;
};

static mutex_t irq_task_lock;

static bool holds_mask(struct irq_line* line) {
    return line->trigger == IRQ_TRIGGER_LEVEL && (line->mode & IRQ_MODE_MASKABLE) && line->mask && line->unmask;
}

static void irq_task_fn(uintptr_t arg) {
    struct irq_task* it = (struct irq_task*)arg;
    struct irq_line* line = it->line;

    while (true) {
        atomic_store(&it->running, true, ATOMIC_SEQ_CST);
        while (atomic_exchange(&it->need, false, ATOMIC_ACQ_REL)) {
            struct irq_handler* handler;
            list_foreach(handler, &line->handlers, next) {
                if (atomic_exchange(&handler->need, false, ATOMIC_ACQ_REL))
                    handler->thread(handler);
            }
            if (atomic_exchange(&it->masked, false, ATOMIC_ACQ_REL))
                line->unmask(line);
        }
        atomic_store(&it->running, false, ATOMIC_SEQ_CST);

        if (!atomic_load(&it->need, ATOMIC_ACQUIRE))
            sched_block(&percpu_get()->sched);
    }
}

static void irq_task_schedule(struct irq_task* it) {
    if (holds_mask(it->line) && !atomic_exchange(&it->masked, true, ATOMIC_ACQ_REL))
        it->line->mask(it->line);
    atomic_store(&it->need, true, ATOMIC_RELEASE);
    sched_wake(it->task);
}

void irq_line_raise(struct irq_line* line) {
    assert(!arch_irq_get_state(), "Raised an IRQ line with interrupts enabled");

    irq_lock();
    line->is_busy = true;

    bool claimed = false;
    bool wake = false;
    struct irq_handler* handler;
    list_foreach(handler, &line->handlers, next) {
        enum irq_status status = handler->raise ? handler->raise(handler) : IRQ_STATUS_WAKE_THREAD;
        if (status == IRQ_STATUS_WAKE_THREAD && handler->thread && line->task) {
            atomic_store(&handler->need, true, ATOMIC_RELEASE);
            wake = true;
            claimed = true;
        } else if (status != IRQ_STATUS_IGNORED) {
            claimed = true;
        }
    }

    if (wake)
        irq_task_schedule(line->task);
    if (!claimed)
        log("irq: Spurious interrupt\n");

    if ((line->mode & IRQ_MODE_EOI) && line->eoi)
        line->eoi(line);

    line->is_busy = false;
    irq_unlock();
}

void irq_line_attach(struct irq_line* line, struct irq_handler* handler) {
    handler->line = line;
    irq_lock();
    list_insert_head(&line->handlers, handler, next);
    irq_unlock();
}

void irq_line_detach(struct irq_line* line, struct irq_handler* handler) {
    irq_lock();
    struct irq_handler** link = &list_first(&line->handlers);
    while (*link && *link != handler)
        link = &list_next(*link, next);
    if (*link)
        *link = list_next(handler, next);
    irq_unlock();

    while (__atomic_load_n(&line->is_busy, __ATOMIC_ACQUIRE))
        arch_cpu_pause();

    atomic_exchange(&handler->need, false, ATOMIC_SEQ_CST);
    if (line->task) {
        while (atomic_load(&line->task->running, ATOMIC_SEQ_CST))
            sched_reschedule(&percpu_get()->sched);
    }
}

errno_t irq_line_attach_task(struct irq_line* line, struct irq_handler* handler, const char* name) {
    if (!handler->thread)
        return EINVAL;

    mutex_lock(&irq_task_lock);
    if (!line->task) {
        struct irq_task* it = mem_alloc(sizeof(*it), 0);
        if (!it) {
            mutex_unlock(&irq_task_lock);
            return ENOMEM;
        }
        it->line = line;

        char task_name[TASK_NAME_MAX];
        ksnprintf(task_name, sizeof(task_name), "irq/%s", name);
        errno_t status = task_create(task_name, process_kernel(), irq_task_fn, (uintptr_t)it, &it->task);
        if (status) {
            mem_free(it, sizeof(*it));
            mutex_unlock(&irq_task_lock);
            return status;
        }
        sched_set_priority(it->task, SCHED_PRIO_IRQ);
        sched_add_task(&percpu_get()->sched, it->task);
        line->task = it;
    }
    mutex_unlock(&irq_task_lock);

    irq_line_attach(line, handler);
    return 0;
}
