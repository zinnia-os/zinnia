// SPDX-License-Identifier: BSD-3-Clause

#pragma once

struct percpu;

void arch_percpu_bsp_init();
void arch_percpu_init();

static inline struct percpu* arch_percpu_get() {
    struct percpu* result;
    asm volatile("mov %0, gs:0" : "=r"(result)::"memory");
    return result;
}
