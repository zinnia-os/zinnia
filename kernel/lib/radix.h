// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/epoch.h>
#include <lib/errno.h>
#include <stddef.h>
#include <stdint.h>

constexpr size_t RADIX_BITS = 6;
constexpr size_t RADIX_SLOTS = 1 << RADIX_BITS;

enum radix_tag : uint8_t {
    RADIX_TAG_DIRTY,
    RADIX_TAG_WRITEBACK,
    RADIX_NUM_TAGS,
};

struct radix_node {
    uint8_t shift;
    uint8_t count;
    uint8_t offset;
    struct radix_node* parent;
    _Atomic(uint64_t) tags[RADIX_NUM_TAGS];
    _Atomic(void*) slots[RADIX_SLOTS];
    struct epoch_head epoch;
};

struct radix_tree {
    _Atomic(struct radix_node*) root;
};

void radix_init(struct radix_tree* tree);
void radix_destroy(struct radix_tree* tree);
bool radix_empty(struct radix_tree* tree);

void* radix_lookup(struct radix_tree* tree, uint64_t index);
errno_t radix_insert(struct radix_tree* tree, uint64_t index, void* value);
void* radix_remove(struct radix_tree* tree, uint64_t index);
void* radix_replace(struct radix_tree* tree, uint64_t index, void* value);

void radix_tag_set(struct radix_tree* tree, uint64_t index, enum radix_tag tag);
void radix_tag_clear(struct radix_tree* tree, uint64_t index, enum radix_tag tag);
bool radix_tag_get(struct radix_tree* tree, uint64_t index, enum radix_tag tag);
bool radix_tagged(struct radix_tree* tree, enum radix_tag tag);

size_t radix_gang_lookup(struct radix_tree* tree, uint64_t first, size_t max, void** out, uint64_t* out_index);
size_t radix_gang_lookup_tag(
    struct radix_tree* tree,
    uint64_t first,
    enum radix_tag tag,
    size_t max,
    void** out,
    uint64_t* out_index
);
