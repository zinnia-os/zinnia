// SPDX-License-Identifier: BSD-3-Clause

#include <lib/epoch.h>
#include <lib/lfqueue.h>
#include <lib/utils.h>
#include <mem/alloc.h>

struct lfqueue_node {
    _Atomic(struct lfqueue_node*) next;
    void* value;
    struct epoch_head epoch;
};

static void node_free_fn(struct epoch_head* head) {
    mem_free(containerof(head, struct lfqueue_node, epoch), sizeof(struct lfqueue_node));
}

static struct lfqueue_node* node_alloc(void* value) {
    struct lfqueue_node* node = mem_alloc(sizeof(struct lfqueue_node), 0);
    if (node)
        node->value = value;
    return node;
}

errno_t lfqueue_init(struct lfqueue* queue) {
    struct lfqueue_node* dummy = node_alloc(nullptr);
    if (!dummy)
        return ENOMEM;
    atomic_init(&queue->head, dummy);
    atomic_init(&queue->tail, dummy);
    return 0;
}

void lfqueue_destroy(struct lfqueue* queue) {
    struct lfqueue_node* node = atomic_load(&queue->head, ATOMIC_ACQUIRE);
    while (node) {
        struct lfqueue_node* next = atomic_load(&node->next, ATOMIC_RELAXED);
        mem_free(node, sizeof(*node));
        node = next;
    }
    atomic_store(&queue->head, nullptr, ATOMIC_RELAXED);
    atomic_store(&queue->tail, nullptr, ATOMIC_RELAXED);
}

bool lfqueue_empty(struct lfqueue* queue) {
    epoch_enter();
    struct lfqueue_node* head = epoch_deref(&queue->head);
    const bool empty = atomic_load(&head->next, ATOMIC_RELAXED) == nullptr;
    epoch_exit();
    return empty;
}

errno_t lfqueue_push(struct lfqueue* queue, void* value) {
    struct lfqueue_node* node = node_alloc(value);
    if (!node)
        return ENOMEM;

    epoch_enter();
    while (true) {
        struct lfqueue_node* tail = epoch_deref(&queue->tail);
        struct lfqueue_node* next = epoch_deref(&tail->next);
        if (tail != atomic_load(&queue->tail, ATOMIC_RELAXED))
            continue;
        if (next) {
            atomic_compare_exchange(&queue->tail, &tail, next, ATOMIC_RELEASE, ATOMIC_RELAXED);
            continue;
        }
        if (atomic_compare_exchange(&tail->next, &next, node, ATOMIC_RELEASE, ATOMIC_RELAXED)) {
            atomic_compare_exchange(&queue->tail, &tail, node, ATOMIC_RELEASE, ATOMIC_RELAXED);
            break;
        }
    }
    epoch_exit();
    return 0;
}

bool lfqueue_pop(struct lfqueue* queue, void** out) {
    epoch_enter();
    while (true) {
        struct lfqueue_node* head = epoch_deref(&queue->head);
        struct lfqueue_node* tail = atomic_load(&queue->tail, ATOMIC_RELAXED);
        struct lfqueue_node* next = epoch_deref(&head->next);
        if (head != atomic_load(&queue->head, ATOMIC_RELAXED))
            continue;
        if (!next) {
            epoch_exit();
            return false;
        }
        if (head == tail) {
            atomic_compare_exchange(&queue->tail, &tail, next, ATOMIC_RELEASE, ATOMIC_RELAXED);
            continue;
        }
        void* value = next->value;
        if (atomic_compare_exchange(&queue->head, &head, next, ATOMIC_RELEASE, ATOMIC_RELAXED)) {
            epoch_call(&head->epoch, node_free_fn);
            epoch_exit();
            *out = value;
            return true;
        }
    }
}
