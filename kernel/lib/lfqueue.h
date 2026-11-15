// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>

struct lfqueue_node;

struct lfqueue {
    _Atomic(struct lfqueue_node*) head;
    _Atomic(struct lfqueue_node*) tail;
};

errno_t lfqueue_init(struct lfqueue* queue);
void lfqueue_destroy(struct lfqueue* queue);
bool lfqueue_empty(struct lfqueue* queue);
errno_t lfqueue_push(struct lfqueue* queue, void* value);
bool lfqueue_pop(struct lfqueue* queue, void** out);
