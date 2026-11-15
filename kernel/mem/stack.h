// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/types.h>
#include <stddef.h>
#include <stdint.h>

struct stack {
    uintptr_t base;
    phys_t phys;
    size_t size;
};

errno_t stack_init(struct stack* ks, size_t size);
void stack_destroy(struct stack* ks);

static inline uintptr_t kernel_stack_top(struct stack* ks) {
    return ks->base + ks->size;
}
