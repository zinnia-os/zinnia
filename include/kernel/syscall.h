#pragma once

#include <kernel/sched.h>

typedef struct {
    union {
        __user void* ptr;
        uintptr_t val;
    };
    errno_t err;
} sc_result_t;

typedef sc_result_t (*syscall_fn_t)(struct arch_context* ctx);

void syscall_dispatch(struct arch_context* ctx);

#define SYSCALL_DEFINE(name, context) sc_result_t syscall_##name(struct arch_context* context)
