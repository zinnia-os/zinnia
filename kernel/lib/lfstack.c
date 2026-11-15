// SPDX-License-Identifier: BSD-3-Clause

#include <lib/epoch.h>
#include <lib/lfstack.h>

void lfstack_init(struct lfstack* stack) {
    atomic_init(&stack->top, nullptr);
}

bool lfstack_empty(struct lfstack* stack) {
    return atomic_load(&stack->top, ATOMIC_RELAXED) == nullptr;
}

void lfstack_push(struct lfstack* stack, struct lfstack_node* node) {
    struct lfstack_node* top = atomic_load(&stack->top, ATOMIC_RELAXED);
    do {
        atomic_store(&node->next, top, ATOMIC_RELAXED);
    } while (!atomic_compare_exchange_weak(&stack->top, &top, node, ATOMIC_RELEASE, ATOMIC_RELAXED));
}

struct lfstack_node* lfstack_pop(struct lfstack* stack) {
    epoch_enter();
    struct lfstack_node* top = epoch_deref(&stack->top);
    while (top) {
        struct lfstack_node* next = atomic_load(&top->next, ATOMIC_RELAXED);
        if (atomic_compare_exchange_weak(&stack->top, &top, next, ATOMIC_ACQUIRE, ATOMIC_ACQUIRE))
            break;
    }
    epoch_exit();
    return top;
}

struct lfstack_node* lfstack_pop_all(struct lfstack* stack) {
    return atomic_exchange(&stack->top, nullptr, ATOMIC_ACQUIRE);
}
