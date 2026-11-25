// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/percpu.h>
#include <bits/irq.h>

// TODO: Instead of disabling irqs, move preempt level to per-task state.

static inline void arch_sched_preempt_disable() {
    bool state = arch_irq_set_state(false);
    percpu_get()->sched.preempt_level++;
    arch_irq_set_state(state);
}

static inline bool arch_sched_preempt_enable() {
    bool state = arch_irq_set_state(false);
    bool zero = --percpu_get()->sched.preempt_level == 0;
    arch_irq_set_state(state);
    return zero;
}
