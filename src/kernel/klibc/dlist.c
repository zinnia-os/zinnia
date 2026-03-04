
// SPDX-FileCopyrightText: 2024-2025 Julian Scheffers <julian@scheffers.net>
// SPDX-FileType: SOURCE
// SPDX-License-Identifier: MIT

#include <kernel/dlist.h>

#include <kernel/assert.h>
#include <kernel/print.h>



// List consistency check.
__attribute__((always_inline)) static inline void consistency_check(struct dlist const *list) {
#ifdef DLIST_CONSISTENCY_CHECK
    size_t              count = 0;
    struct dlist_node const *node  = list->head;
    while (node) {
        count++;
        if (count > list->len) {
            kprintf("BUG: List length mismatch: %zu vs %zu\n", count, list->len);
            abort();
        }
        node = node->next;
    }
    DEBUG_ASSERT(count == list->len, "List length mismatch: %zu vs %zu", count, list->len);

    count = 0;
    node  = list->tail;
    while (node) {
        count++;
        if (count > list->len) {
            kprintf("BUG: List length mismatch: %zu vs %zu\n", count, list->len);
            abort();
        }
        node = node->previous;
    }
    DEBUG_ASSERT(count == list->len, "List length mismatch: %zu vs %zu", count, list->len);
#else
    (void)list;
#endif
}

// Concatenates the elements from dlist `back` on dlist `front`, clearing `back` in the process.
// Both lists must be non-NULL.
void dlist_concat(struct dlist *front, struct dlist *back) {
    DEBUG_ASSERT(front != nullptr, "List front is nullptr");
    DEBUG_ASSERT(back != nullptr, "List back is nullptr");
    consistency_check(front);
    consistency_check(back);

    if (front->tail != nullptr && back->tail != nullptr) {
        // Both lists have elements.
        // Concatenate lists.
        front->tail->next     = back->head;
        back->head->previous  = front->tail;
        front->tail           = back->tail;
        front->len           += back->len;
        *back                 = DLIST_EMPTY;

    } else if (front->tail != nullptr) {
        // Front list has elements, back is empty.
        // No action needed.
        DEBUG_ASSERT(back->head == nullptr, "Back list head should be nullptr");
        DEBUG_ASSERT(back->len == 0, "Back len should be 0");

    } else if (back->tail != nullptr) {
        // Front list is empty, back has elements.
        // Move all elements to front list.
        DEBUG_ASSERT(front->head == nullptr, "Front list head should be nullptr");
        DEBUG_ASSERT(front->len == 0, "Front len should be 0");
        *front = *back;
        *back  = DLIST_EMPTY;

    } else {
        // Both lists are empty.
        // No action needed.
        DEBUG_ASSERT(back->head == nullptr, "Back list head should be nullptr");
        DEBUG_ASSERT(back->len == 0, "Back len should be 0");
        DEBUG_ASSERT(front->head == nullptr, "Front list head should be nullptr");
        DEBUG_ASSERT(front->len == 0, "Front len should be 0");
    }
    consistency_check(front);
}

// Appends `node` after the `tail` of the `list`.
// `node` must not be in `list` already.
// Both `list` and `node` must be non-NULL.
void dlist_append(struct dlist *const list, struct dlist_node *const node) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    DEBUG_ASSERT(node != nullptr, "Node is nullptr");
    DEBUG_ASSERT(node->next == nullptr, "Node is already in a list");
    DEBUG_ASSERT(node->previous == nullptr, "Node is already in a list");
    consistency_check(list);
    DEBUG_ASSERT(!dlist_contains(list, node), "List already contains node");

    *node = (struct dlist_node){
        .next     = nullptr,
        .previous = list->tail,
    };

    if (list->tail != nullptr) {
        list->tail->next = node;
    } else {
        DEBUG_ASSERT(list->head == nullptr, "");
        DEBUG_ASSERT(list->len == 0, "");
        list->head = node;
    }
    list->tail  = node;
    list->len  += 1;
    consistency_check(list);
}

// Prepends `node` before the `head` of the `list`.
// `node` must not be in `list` already.
// Both `list` and `node` must be non-NULL.
void dlist_prepend(struct dlist *const list, struct dlist_node *const node) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    DEBUG_ASSERT(node != nullptr, "Node is nullptr");
    DEBUG_ASSERT(node->next == nullptr, "Node is already in a list");
    DEBUG_ASSERT(node->previous == nullptr, "Node is already in a list");
    consistency_check(list);
    DEBUG_ASSERT(!dlist_contains(list, node), "List already contains node");

    *node = (struct dlist_node){
        .next     = list->head,
        .previous = nullptr,
    };

    if (list->head != nullptr) {
        list->head->previous = node;
    } else {
        DEBUG_ASSERT(list->tail == nullptr, "");
        DEBUG_ASSERT(list->len == 0, "");
        list->tail = node;
    }
    list->head  = node;
    list->len  += 1;
    consistency_check(list);
}

// Inserts `node` after `existing` in `list`.
// `node` must not be in `list` already and `existing` must be in `list` already.
// `list`, `node` and `existing` must be non-NULL.
void dlist_insert_after(struct dlist *list, struct dlist_node *existing, struct dlist_node *node) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    DEBUG_ASSERT(node != nullptr, "Node is nullptr");
    DEBUG_ASSERT(node->next == nullptr, "Node is already in a list");
    DEBUG_ASSERT(node->previous == nullptr, "Node is already in a list");
    consistency_check(list);
    DEBUG_ASSERT(!dlist_contains(list, node), "List already contains node");
    DEBUG_ASSERT(dlist_contains(list, existing), "Existing node not in this list");

    *node = (struct dlist_node){
        .next     = existing->next,
        .previous = existing,
    };
    if (existing->next) {
        existing->next->previous = node;
    } else {
        list->tail = node;
    }
    existing->next  = node;
    list->len      += 1;
    consistency_check(list);
}

// Inserts `node` before `existing` in `list`.
// `node` must not be in `list` already and `existing` must be in `list` already.
// `list`, `node` and `existing` must be non-NULL.
void dlist_insert_before(struct dlist *list, struct dlist_node *existing, struct dlist_node *node) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    DEBUG_ASSERT(node != nullptr, "Node is nullptr");
    DEBUG_ASSERT(node->next == nullptr, "Node is already in a list");
    DEBUG_ASSERT(node->previous == nullptr, "Node is already in a list");
    consistency_check(list);
    DEBUG_ASSERT(!dlist_contains(list, node), "List already contains node");
    DEBUG_ASSERT(dlist_contains(list, existing), "Existing node not in this list");

    *node = (struct dlist_node){
        .next     = existing,
        .previous = existing->previous,
    };
    if (existing->previous) {
        existing->previous->next = node;
    } else {
        list->head = node;
    }
    existing->previous  = node;
    list->len          += 1;
    consistency_check(list);
}

// Removes the `head` of the given `list`. Will return nullptr if the list was empty.
// `list` must be non-NULL.
struct dlist_node *dlist_pop_front(struct dlist *const list) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    consistency_check(list);

    if (list->head != nullptr) {
        DEBUG_ASSERT(list->tail != nullptr, "List is corrupted");
        DEBUG_ASSERT(list->len > 0, "List is corrupted");

        struct dlist_node *const node = list->head;

        if (node->next != nullptr) {
            node->next->previous = node->previous;
        }

        list->len  -= 1;
        list->head  = node->next;
        if (list->head == nullptr) {
            list->tail = nullptr;
        }

        DEBUG_ASSERT((list->head != nullptr) == (list->tail != nullptr), "List is corrupted");
        DEBUG_ASSERT((list->head != nullptr) == (list->len > 0), "List is corrupted");

        *node = DLIST_NODE_EMPTY;
        consistency_check(list);
        return node;
    } else {
        DEBUG_ASSERT(list->tail == nullptr, "List is corrupted");
        DEBUG_ASSERT(list->len == 0, "List is corrupted");
        return nullptr;
    }
}

// Removes the `tail` of the given `list`. Will return nullptr if the list was empty.
// `list` must be non-NULL.
struct dlist_node *dlist_pop_back(struct dlist *const list) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    consistency_check(list);

    if (list->tail != nullptr) {
        DEBUG_ASSERT(list->head != nullptr, "List is corrupted");
        DEBUG_ASSERT(list->len > 0, "List is corrupted");

        struct dlist_node *const node = list->tail;

        if (node->previous != nullptr) {
            node->previous->next = node->next;
        }

        list->len  -= 1;
        list->tail  = node->previous;
        if (list->tail == nullptr) {
            list->head = nullptr;
        }

        DEBUG_ASSERT((list->head != nullptr) == (list->tail != nullptr), "List is corrupted");
        DEBUG_ASSERT((list->head != nullptr) == (list->len > 0), "List is corrupted");

        *node = DLIST_NODE_EMPTY;
        consistency_check(list);
        return node;
    } else {
        DEBUG_ASSERT(list->head == nullptr, "List is corrupted");
        DEBUG_ASSERT(list->len == 0, "List is corrupted");
        return nullptr;
    }
}

// Checks if `list` contains the given `node`.
// Both `list` and `node` must be non-NULL.
bool dlist_contains(struct dlist const *const list, struct dlist_node const *const node) {
    DEBUG_ASSERT(list != nullptr, "List is nullptr");
    DEBUG_ASSERT(node != nullptr, "Node is nullptr");
    consistency_check(list);

    struct dlist_node const *iter = list->head;
    while (iter != nullptr) {
        if (iter == node) {
            return true;
        }
        iter = iter->next;
    }

    return false;
}

// Removes `node` from `list`. `node` must be either an empty (non-inserted) node or must be contained in `list`.
// Both `list` and `node` must be non-NULL.
void dlist_remove(struct dlist *const list, struct dlist_node *const node) {
    DEBUG_ASSERT(list->len > 0, "List must not be empty");
    consistency_check(list);
    DEBUG_ASSERT(dlist_contains(list, node), "List must contain node");

    if (node->previous != nullptr) {
        node->previous->next = node->next;
    }
    if (node->next != nullptr) {
        node->next->previous = node->previous;
    }

    if (node == list->head) {
        list->head = node->next;
    }
    if (node == list->tail) {
        list->tail = node->previous;
    }

    list->len -= 1;
    *node      = DLIST_NODE_EMPTY;

    consistency_check(list);
}
