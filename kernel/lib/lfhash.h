// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>
#include <lib/errno.h>
#include <stddef.h>
#include <stdint.h>

constexpr size_t LFHASH_SEGMENT_BUCKETS = 512;
constexpr size_t LFHASH_SEGMENTS = 1024;
constexpr size_t LFHASH_MIN_BUCKETS = 16;
constexpr size_t LFHASH_LOAD_FACTOR = 2;

struct lfhash_node {
    _Atomic(uintptr_t) next;
    uint64_t key;
};

struct lfhash_bucket;

struct lfhash {
    _Atomic(struct lfhash_bucket*)* segments;
    atomic_size_t size;
    atomic_size_t count;
};

typedef bool (*lfhash_eq_fn)(const struct lfhash_node* node, const void* key);

errno_t lfhash_init(struct lfhash* table);
void lfhash_destroy(struct lfhash* table);
size_t lfhash_count(struct lfhash* table);

errno_t lfhash_insert(struct lfhash* table, uint64_t hash, struct lfhash_node* node, lfhash_eq_fn eq, const void* key);
struct lfhash_node* lfhash_lookup(struct lfhash* table, uint64_t hash, lfhash_eq_fn eq, const void* key);
struct lfhash_node* lfhash_remove(struct lfhash* table, uint64_t hash, lfhash_eq_fn eq, const void* key);

struct lfhash_node* lfhash_first(struct lfhash* table);
struct lfhash_node* lfhash_next(struct lfhash_node* node);

#define lfhash_foreach(table, var) for ((var) = lfhash_first(table); (var); (var) = lfhash_next(var))
