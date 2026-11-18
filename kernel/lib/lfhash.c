// SPDX-License-Identifier: BSD-3-Clause

#include <lib/cpu.h>
#include <lib/epoch.h>
#include <lib/lfhash.h>
#include <mem/alloc.h>

constexpr uintptr_t LFHASH_MARK = 1;
constexpr uint64_t LFHASH_MSB = (uint64_t)1 << 63;

enum bucket_state : uint8_t {
    BUCKET_UNLINKED,
    BUCKET_LINKING,
    BUCKET_LINKED,
};

struct lfhash_bucket {
    struct lfhash_node node;
    _Atomic(enum bucket_state) state;
};

struct find {
    _Atomic(uintptr_t)* prev;
    struct lfhash_node* cur;
};

static uint64_t bitrev(uint64_t x) {
    x = ((x >> 1) & 0x5555555555555555) | ((x & 0x5555555555555555) << 1);
    x = ((x >> 2) & 0x3333333333333333) | ((x & 0x3333333333333333) << 2);
    x = ((x >> 4) & 0x0F0F0F0F0F0F0F0F) | ((x & 0x0F0F0F0F0F0F0F0F) << 4);
    return __builtin_bswap64(x);
}

static uint64_t regular_key(uint64_t hash) {
    return bitrev(hash | LFHASH_MSB);
}

static uint64_t sentinel_key(size_t bucket) {
    return bitrev(bucket);
}

static size_t parent_of(size_t bucket) {
    return bucket & ~((size_t)1 << (63 - __builtin_clzll(bucket)));
}

static struct lfhash_node* node_of(uintptr_t link) {
    return (struct lfhash_node*)(link & ~LFHASH_MARK);
}

static struct lfhash_bucket* bucket_get(struct lfhash* table, size_t bucket) {
    struct lfhash_bucket* segment = atomic_load(&table->segments[bucket / LFHASH_SEGMENT_BUCKETS], ATOMIC_ACQUIRE);
    return segment ? &segment[bucket % LFHASH_SEGMENT_BUCKETS] : nullptr;
}

static struct lfhash_bucket* segment_ensure(struct lfhash* table, size_t bucket) {
    _Atomic(struct lfhash_bucket*)* slot = &table->segments[bucket / LFHASH_SEGMENT_BUCKETS];
    struct lfhash_bucket* segment = atomic_load(slot, ATOMIC_ACQUIRE);
    if (!segment) {
        struct lfhash_bucket* fresh = mem_alloc(sizeof(struct lfhash_bucket) * LFHASH_SEGMENT_BUCKETS, 0);
        if (!fresh)
            return nullptr;
        if (atomic_compare_exchange(slot, &segment, fresh, ATOMIC_ACQ_REL, ATOMIC_ACQUIRE))
            segment = fresh;
        else
            mem_free(fresh, sizeof(struct lfhash_bucket) * LFHASH_SEGMENT_BUCKETS);
    }
    return &segment[bucket % LFHASH_SEGMENT_BUCKETS];
}

static bool find(_Atomic(uintptr_t)* head, uint64_t key, lfhash_eq_fn eq, const void* eq_key, struct find* out) {
again:
    _Atomic(uintptr_t)* prev = head;
    struct lfhash_node* cur = node_of(atomic_load(prev, ATOMIC_ACQUIRE));
    while (cur) {
        const uintptr_t next = atomic_load(&cur->next, ATOMIC_ACQUIRE);
        if (next & LFHASH_MARK) {
            uintptr_t expected = (uintptr_t)cur;
            if (!atomic_compare_exchange(prev, &expected, next & ~LFHASH_MARK, ATOMIC_RELEASE, ATOMIC_RELAXED))
                goto again;
            cur = node_of(next);
            continue;
        }
        if (cur->key > key)
            break;
        if (cur->key == key && (!eq || eq(cur, eq_key))) {
            *out = (struct find){.prev = prev, .cur = cur};
            return true;
        }
        prev = &cur->next;
        cur = node_of(next);
    }
    *out = (struct find){.prev = prev, .cur = cur};
    return false;
}

static void bucket_link(struct lfhash* table, size_t index, struct lfhash_bucket* bucket) {
    enum bucket_state state = BUCKET_UNLINKED;
    if (!atomic_compare_exchange(&bucket->state, &state, BUCKET_LINKING, ATOMIC_RELAXED, ATOMIC_RELAXED)) {
        while (atomic_load(&bucket->state, ATOMIC_ACQUIRE) != BUCKET_LINKED)
            cpu_pause();
        return;
    }

    const size_t parent_index = parent_of(index);
    struct lfhash_bucket* parent = bucket_get(table, parent_index);
    if (atomic_load(&parent->state, ATOMIC_ACQUIRE) != BUCKET_LINKED)
        bucket_link(table, parent_index, parent);

    bucket->node.key = sentinel_key(index);
    while (true) {
        struct find f;
        find(&parent->node.next, bucket->node.key, nullptr, nullptr, &f);
        atomic_store(&bucket->node.next, (uintptr_t)f.cur, ATOMIC_RELAXED);
        uintptr_t expected = (uintptr_t)f.cur;
        if (atomic_compare_exchange(f.prev, &expected, (uintptr_t)&bucket->node, ATOMIC_RELEASE, ATOMIC_RELAXED))
            break;
    }
    atomic_store(&bucket->state, BUCKET_LINKED, ATOMIC_RELEASE);
}

static bool ensure_segments(struct lfhash* table, size_t index) {
    while (index) {
        if (!segment_ensure(table, index))
            return false;
        index = parent_of(index);
    }
    return true;
}

static _Atomic(uintptr_t)* start_of(struct lfhash* table, size_t index) {
    while (true) {
        struct lfhash_bucket* bucket = bucket_get(table, index);
        if (bucket && atomic_load(&bucket->state, ATOMIC_ACQUIRE) == BUCKET_LINKED)
            return &bucket->node.next;
        index = parent_of(index);
    }
}

static size_t index_of(struct lfhash* table, uint64_t hash) {
    return hash & (atomic_load(&table->size, ATOMIC_RELAXED) - 1);
}

errno_t lfhash_init(struct lfhash* table) {
    table->segments = mem_alloc(sizeof(*table->segments) * LFHASH_SEGMENTS, 0);
    if (!table->segments)
        return ENOMEM;
    struct lfhash_bucket* root = segment_ensure(table, 0);
    if (!root) {
        mem_free(table->segments, sizeof(*table->segments) * LFHASH_SEGMENTS);
        return ENOMEM;
    }
    root->node.key = sentinel_key(0);
    atomic_init(&root->node.next, 0);
    atomic_init(&root->state, BUCKET_LINKED);
    atomic_init(&table->size, LFHASH_MIN_BUCKETS);
    atomic_init(&table->count, 0);
    return 0;
}

void lfhash_destroy(struct lfhash* table) {
    for (size_t i = 0; i < LFHASH_SEGMENTS; i++)
        mem_free(
            atomic_load(&table->segments[i], ATOMIC_RELAXED),
            sizeof(struct lfhash_bucket) * LFHASH_SEGMENT_BUCKETS
        );
    mem_free(table->segments, sizeof(*table->segments) * LFHASH_SEGMENTS);
    table->segments = nullptr;
}

size_t lfhash_count(struct lfhash* table) {
    return atomic_load(&table->count, ATOMIC_RELAXED);
}

static void maybe_grow(struct lfhash* table, size_t count) {
    size_t size = atomic_load(&table->size, ATOMIC_RELAXED);
    if (count / size > LFHASH_LOAD_FACTOR && size * 2 <= LFHASH_SEGMENTS * LFHASH_SEGMENT_BUCKETS)
        atomic_compare_exchange(&table->size, &size, size * 2, ATOMIC_RELAXED, ATOMIC_RELAXED);
}

errno_t lfhash_insert(struct lfhash* table, uint64_t hash, struct lfhash_node* node, lfhash_eq_fn eq, const void* key) {
    node->key = regular_key(hash);
    const size_t index = index_of(table, hash);
    const bool linkable = ensure_segments(table, index);

    epoch_enter();
    if (linkable) {
        struct lfhash_bucket* bucket = bucket_get(table, index);
        if (atomic_load(&bucket->state, ATOMIC_ACQUIRE) != BUCKET_LINKED)
            bucket_link(table, index, bucket);
    }

    _Atomic(uintptr_t)* start = start_of(table, index);
    while (true) {
        struct find f;
        if (find(start, node->key, eq, key, &f)) {
            epoch_exit();
            return EEXIST;
        }
        atomic_store(&node->next, (uintptr_t)f.cur, ATOMIC_RELAXED);
        uintptr_t expected = (uintptr_t)f.cur;
        if (atomic_compare_exchange(f.prev, &expected, (uintptr_t)node, ATOMIC_RELEASE, ATOMIC_RELAXED))
            break;
    }
    epoch_exit();

    maybe_grow(table, atomic_fetch_add(&table->count, 1, ATOMIC_RELAXED) + 1);
    return 0;
}

struct lfhash_node* lfhash_lookup(struct lfhash* table, uint64_t hash, lfhash_eq_fn eq, const void* key) {
    struct find f;
    return find(start_of(table, index_of(table, hash)), regular_key(hash), eq, key, &f) ? f.cur : nullptr;
}

struct lfhash_node* lfhash_remove(struct lfhash* table, uint64_t hash, lfhash_eq_fn eq, const void* key) {
    const uint64_t so_key = regular_key(hash);
    struct lfhash_node* victim = nullptr;

    epoch_enter();
    _Atomic(uintptr_t)* start = start_of(table, index_of(table, hash));
    while (true) {
        struct find f;
        if (!find(start, so_key, eq, key, &f))
            break;
        uintptr_t next = atomic_load(&f.cur->next, ATOMIC_ACQUIRE);
        if (next & LFHASH_MARK)
            continue;
        if (!atomic_compare_exchange(&f.cur->next, &next, next | LFHASH_MARK, ATOMIC_RELAXED, ATOMIC_RELAXED))
            continue;
        victim = f.cur;
        uintptr_t expected = (uintptr_t)victim;
        if (!atomic_compare_exchange(f.prev, &expected, next, ATOMIC_RELEASE, ATOMIC_RELAXED))
            find(start, so_key, eq, key, &f);
        break;
    }
    epoch_exit();

    if (victim)
        atomic_fetch_sub(&table->count, 1, ATOMIC_RELAXED);
    return victim;
}

static struct lfhash_node* skip(uintptr_t link) {
    struct lfhash_node* node = node_of(link);
    while (node) {
        const uintptr_t next = atomic_load(&node->next, ATOMIC_ACQUIRE);
        if (!(next & LFHASH_MARK) && (node->key & 1))
            return node;
        node = node_of(next);
    }
    return nullptr;
}

struct lfhash_node* lfhash_first(struct lfhash* table) {
    return skip(atomic_load(&bucket_get(table, 0)->node.next, ATOMIC_ACQUIRE));
}

struct lfhash_node* lfhash_next(struct lfhash_node* node) {
    return skip(atomic_load(&node->next, ATOMIC_ACQUIRE));
}
