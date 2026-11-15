// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/sched.h>

struct syscall_ret {
    uintptr_t value;
    int error;
};

struct syscall {
    void* func;
    int num_args;
};

void syscall_dispatch(arch_context_t* ctx);

extern const struct syscall syscall_table[256];
extern const char* const syscall_names[256];
