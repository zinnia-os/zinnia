// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>

struct lfstack_node {
    _Atomic(struct lfstack_node*) next;
};

struct lfstack {
    _Atomic(struct lfstack_node*) top;
};

void lfstack_init(struct lfstack* stack);
bool lfstack_empty(struct lfstack* stack);
void lfstack_push(struct lfstack* stack, struct lfstack_node* node);
struct lfstack_node* lfstack_pop(struct lfstack* stack);
struct lfstack_node* lfstack_pop_all(struct lfstack* stack);
