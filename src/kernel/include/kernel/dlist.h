
// SPDX-FileCopyrightText: 2024-2025 Julian Scheffers <julian@scheffers.net>
// SPDX-FileType: SOURCE
// SPDX-License-Identifier: MIT

#pragma once

#include <stddef.h>

// A node of a doubly linked list structure. It contains no data, use
// `field_parent_ptr` from `meta.h` to obtain the containing structure.
struct dlist_node {
    // Pointer to the next item in the linked list.
    struct dlist_node* next;
    // Pointer to the previous item in the linked list.
    struct dlist_node* previous;
};

// A doubly linekd list.
struct dlist {
    // Current number of elements in the list.
    size_t len;
    // Pointer to the first node in the list or NULL if the list is empty.
    struct dlist_node* head;
    // Pointer to the last node in the list or NULL if the list is empty.
    struct dlist_node* tail;
};

// Initializer value for an empty list. Convenience macro for
// zero-initialization.
#define DLIST_EMPTY ((struct dlist){.len = 0, .head = nullptr, .tail = nullptr})

// Initializer value for a list node. Convenience macro for zero-initialization.
#define DLIST_NODE_EMPTY ((struct dlist_node){.next = nullptr, .previous = nullptr})

#if defined(__has_builtin)
#if __has_builtin(__builtin_types_compatible_p)
// Get pointer to parent object given pointer to a struct or union member.
#define container_of(ptr, type, member) \
    ({ \
        _Static_assert( \
            __builtin_types_compatible_p(*(ptr), ((type*)0)->member), \
            "Incompatible types for container_of" \
        ); \
        (type*)((size_t)ptr - offsetof(type, member)); \
    })
#endif
#endif
#ifndef container_of
// Get pointer to parent object given pointer to a struct or union member.
#define container_of(ptr, type, member) (type*)((size_t)ptr - offsetof(type, member))
#endif

// Generate a foreach loop for a dlist.
#define dlist_foreach(type, varname, nodename, list) \
    for (type* varname = container_of((list)->head, type, nodename); &varname->nodename; \
         varname = container_of(varname->nodename.next, type, nodename))

// Generate a foreach loop for a dlist where the node name is `node`.
#define dlist_foreach_node(type, varname, list) dlist_foreach(type, varname, node, list)

// Concatenates the elements from dlist `back` on dlist `front`, clearing `back` in the process.
// Both lists must be non-NULL.
void dlist_concat(struct dlist* front, struct dlist* back);

// Appends `node` after the `tail` of the `list`.
// `node` must not be in `list` already.
// Both `list` and `node` must be non-NULL.
void dlist_append(struct dlist* list, struct dlist_node* node);

// Prepends `node` before the `head` of the `list`.
// `node` must not be in `list` already.
// Both `list` and `node` must be non-NULL.
void dlist_prepend(struct dlist* list, struct dlist_node* node);

// Inserts `node` after `existing` in `list`.
// `node` must not be in `list` already and `existing` must be in `list` already.
// `list`, `node` and `existing` must be non-NULL.
void dlist_insert_after(struct dlist* list, struct dlist_node* existing, struct dlist_node* node);

// Inserts `node` before `existing` in `list`.
// `node` must not be in `list` already and `existing` must be in `list` already.
// `list`, `node` and `existing` must be non-NULL.
void dlist_insert_before(struct dlist* list, struct dlist_node* existing, struct dlist_node* node);

// Removes the `head` of the given `list`. Will return NULL if the list was empty.
// `list` must be non-NULL.
struct dlist_node* dlist_pop_front(struct dlist* list);

// Removes the `tail` of the given `list`. Will return NULL if the list was empty.
// `list` must be non-NULL.
struct dlist_node* dlist_pop_back(struct dlist* list);

// Checks if `list` contains the given `node`.
// Both `list` and `node` must be non-NULL.
bool dlist_contains(struct dlist const* list, struct dlist_node const* node);

// Removes `node` from `list`. `node` must be either an empty (non-inserted) node or must be contained in `list`.
// Both `list` and `node` must be non-NULL.
void dlist_remove(struct dlist* list, struct dlist_node* node);
