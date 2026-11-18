// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/percpu.h>
#include <stddef.h>

static inline void arch_sched_preempt_disable() {
    asm volatile("inc qword ptr gs:%c0" ::"i"(offsetof(struct percpu, sched.preempt_level)) : "memory");
}

static inline bool arch_sched_preempt_enable() {
    bool zero;
    asm volatile("dec qword ptr gs:%c1" : "=@ccz"(zero) : "i"(offsetof(struct percpu, sched.preempt_level)) : "memory");
    return zero;
}
