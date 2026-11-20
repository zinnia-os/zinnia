// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/initgraph.h>
#include <lib/sched.h>
#include <mem/stack.h>
#include <mem/usercopy.h>
#include <bits/percpu.h>
#include <stddef.h>
#include <stdint.h>

// # On using per-CPU data
//
// Defining variables with `[[__percpu]]` will put them in a special section.
// This section is unique for every CPU. To access the correct memory of the
// caller's CPU, use `percpu_ref`. To access another CPU's copy, use `percpu_ref_on`.
//
// ## Example
//
// ```
// [[__percpu]]
// int my_cpu_data = 0;
//
// int get_foo() {
//     return *percpu_ref(my_cpu_data);
// }
// ```

#define __percpu              __used, __section(".percpu.data")
#define percpu_ref_on(cpu, x) ((typeof(&(x)))((void*)(cpu) + ((void*)&(x) - (void*)&percpu_bsp)))
#define percpu_ref(x)         percpu_ref_on(percpu_get(), x)

// CPU-relative information.
struct percpu {
    struct percpu* self;    // A pointer to this structure.
    size_t id;              // The virtual ID of this CPU.
    bool online;            // Whether this CPU is initialized and active.
    uintptr_t kernel_stack; // The kernel mode stack.
    uintptr_t user_stack;   // The user mode stack.
    struct scheduler sched; // Scheduler information.
    struct stack boot_stack;
};

extern uint8_t __ld_percpu_start[];
extern uint8_t __ld_percpu_end[];

static inline size_t percpu_stride() {
    return __ld_percpu_end - __ld_percpu_start;
}

static inline struct percpu* percpu_cpu(size_t id) {
    return (struct percpu*)(__ld_percpu_start + id * percpu_stride());
}

struct percpu* percpu_new();
size_t percpu_max_cpus();
size_t percpu_count();
static inline struct percpu* percpu_get() {
    return arch_percpu_get();
}
void percpu_bsp_init();
void percpu_init(struct boot_info* info);

INIT_EXTERN(percpu_stage);

// Per-CPU data for the bootstrap processor.
extern struct percpu percpu_bsp;
