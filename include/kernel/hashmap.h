#pragma once

#include <kernel/alloc.h>
#include <kernel/print.h>
#include <uapi/errno.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

// FNV-1a hash over raw bytes.
static inline uint64_t hashmap_hash_default(const void* key, size_t len) {
    const uint8_t* data = (const uint8_t*)key;
    uint64_t hash = 0xcbf29ce484222325;
    for (size_t i = 0; i < len; i++) {
        hash ^= data[i];
        hash *= 0x100000001b3;
    }
    return hash;
}

static inline uint64_t hashmap_hash_string(const void* key, size_t) {
    const char* buf = *(const char**)key;
    return hashmap_hash_default(buf, strlen(buf));
}

// Byte-wise equality.
static inline bool hashmap_eq_default(const void* a, const void* b, size_t len) {
    return memcmp(a, b, len) == 0;
}

static inline bool hashmap_eq_string(const void* a, const void* b, size_t) {
    const char* buf_a = *(const char**)a;
    const char* buf_b = *(const char**)b;
    return strcmp(buf_a, buf_b) == 0;
}

// Ensure a hash is never zero (zero is the empty sentinel).
#define _HASHMAP_FIXHASH(h) ((h) == 0 ? (uint64_t)1 : (h))

#define HASHMAP(K, V) \
    struct { \
        K* keys; \
        V* values; \
        uint64_t* hashes; \
        size_t capacity; \
        size_t count; \
    }

#define HASHMAP_INIT(map) \
    do { \
        (map)->keys = nullptr; \
        (map)->values = nullptr; \
        (map)->hashes = nullptr; \
        (map)->capacity = 0; \
        (map)->count = 0; \
    } while (0)

#define HASHMAP_DESTROY(map) \
    do { \
        mem_free((map)->keys); \
        mem_free((map)->values); \
        mem_free((map)->hashes); \
        (map)->keys = nullptr; \
        (map)->values = nullptr; \
        (map)->hashes = nullptr; \
        (map)->capacity = 0; \
        (map)->count = 0; \
    } while (0)

#define HASHMAP_COUNT(map) ((map)->count)

// Internal: probe distance from ideal position.
#define _HASHMAP_PROBE_DIST(hash, pos, cap) (((pos) - ((hash) & ((cap) - 1))) & ((cap) - 1))

// Internal: grow and rehash.
#define _HASHMAP_GROW(map, hash_fn, eq_fn) \
    do { \
        size_t _old_cap = (map)->capacity; \
        size_t _new_cap = _old_cap == 0 ? 16 : _old_cap * 2; \
        typeof((map)->keys) _old_keys = (map)->keys; \
        typeof((map)->values) _old_values = (map)->values; \
        uint64_t* _old_hashes = (map)->hashes; \
        (map)->keys = mem_alloc(_new_cap * sizeof(*(map)->keys), ALLOC_NOZERO); \
        (map)->values = mem_alloc(_new_cap * sizeof(*(map)->values), ALLOC_NOZERO); \
        (map)->hashes = mem_alloc(_new_cap * sizeof(uint64_t), 0); \
        (map)->capacity = _new_cap; \
        (map)->count = 0; \
        for (size_t _i = 0; _i < _old_cap; _i++) { \
            if (_old_hashes[_i] != 0) { \
                _HASHMAP_INSERT_INNER(map, _old_keys[_i], _old_values[_i], _old_hashes[_i]); \
            } \
        } \
        mem_free(_old_keys); \
        mem_free(_old_values); \
        mem_free(_old_hashes); \
    } while (0)

// Internal: insert with a precomputed hash. Used by grow and the public INSERT macro.
#define _HASHMAP_INSERT_INNER(map, k, v, h) \
    do { \
        size_t _cap = (map)->capacity; \
        size_t _pos = (h) & (_cap - 1); \
        typeof(*(map)->keys) _ik = (k); \
        typeof(*(map)->values) _iv = (v); \
        uint64_t _ih = (h); \
        for (;;) { \
            if ((map)->hashes[_pos] == 0) { \
                (map)->keys[_pos] = _ik; \
                (map)->values[_pos] = _iv; \
                (map)->hashes[_pos] = _ih; \
                (map)->count++; \
                break; \
            } \
            size_t _existing_dist = _HASHMAP_PROBE_DIST((map)->hashes[_pos], _pos, _cap); \
            size_t _insert_dist = _HASHMAP_PROBE_DIST(_ih, _pos, _cap); \
            if (_insert_dist > _existing_dist) { \
                typeof(*(map)->keys) _tk = (map)->keys[_pos]; \
                typeof(*(map)->values) _tv = (map)->values[_pos]; \
                uint64_t _th = (map)->hashes[_pos]; \
                (map)->keys[_pos] = _ik; \
                (map)->values[_pos] = _iv; \
                (map)->hashes[_pos] = _ih; \
                _ik = _tk; \
                _iv = _tv; \
                _ih = _th; \
            } \
            _pos = (_pos + 1) & (_cap - 1); \
        } \
    } while (0)

// Insert or update a key-value pair. Returns 0 or ENOMEM.
#define HASHMAP_INSERT(map, key, val, hash_fn, eq_fn) \
    ({ \
        errno_t _status = 0; \
        typeof(*(map)->keys) _k = (key); \
        uint64_t _h = _HASHMAP_FIXHASH((hash_fn)(&_k, sizeof(_k))); \
        /* Check for existing key first. */ \
        bool _found = false; \
        if ((map)->capacity > 0) { \
            size_t _cap = (map)->capacity; \
            size_t _pos = _h & (_cap - 1); \
            for (size_t _d = 0; _d < _cap; _d++) { \
                if ((map)->hashes[_pos] == 0) \
                    break; \
                if (_HASHMAP_PROBE_DIST((map)->hashes[_pos], _pos, _cap) < _d) \
                    break; \
                if ((map)->hashes[_pos] == _h && (eq_fn)(&(map)->keys[_pos], &_k, sizeof(_k))) { \
                    (map)->values[_pos] = (val); \
                    _found = true; \
                    break; \
                } \
                _pos = (_pos + 1) & (_cap - 1); \
            } \
        } \
        if (!_found) { \
            if ((map)->count * 4 >= (map)->capacity * 3 || (map)->capacity == 0) { \
                _HASHMAP_GROW(map, hash_fn, eq_fn); \
                if ((map)->keys == nullptr || (map)->values == nullptr || (map)->hashes == nullptr) { \
                    _status = ENOMEM; \
                } \
            } \
            if (_status == 0) { \
                _HASHMAP_INSERT_INNER(map, _k, (val), _h); \
            } \
        } \
        _status; \
    })

// Look up a key. Returns a pointer to the value, or nullptr if not found.
#define HASHMAP_GET(map, key, hash_fn, eq_fn) \
    ({ \
        typeof(*(map)->values)* _result = nullptr; \
        if ((map)->capacity > 0) { \
            typeof(*(map)->keys) _k = (key); \
            uint64_t _h = _HASHMAP_FIXHASH((hash_fn)(&_k, sizeof(_k))); \
            size_t _cap = (map)->capacity; \
            size_t _pos = _h & (_cap - 1); \
            for (size_t _d = 0; _d < _cap; _d++) { \
                if ((map)->hashes[_pos] == 0) \
                    break; \
                if (_HASHMAP_PROBE_DIST((map)->hashes[_pos], _pos, _cap) < _d) \
                    break; \
                if ((map)->hashes[_pos] == _h && (eq_fn)(&(map)->keys[_pos], &_k, sizeof(_k))) { \
                    _result = &(map)->values[_pos]; \
                    break; \
                } \
                _pos = (_pos + 1) & (_cap - 1); \
            } \
        } \
        _result; \
    })

// Remove a key. Returns true if the key was found and removed.
// Uses backward-shift deletion to avoid tombstones.
#define HASHMAP_REMOVE(map, key, hash_fn, eq_fn) \
    ({ \
        bool _removed = false; \
        if ((map)->capacity > 0) { \
            typeof(*(map)->keys) _k = (key); \
            uint64_t _h = _HASHMAP_FIXHASH((hash_fn)(&_k, sizeof(_k))); \
            size_t _cap = (map)->capacity; \
            size_t _pos = _h & (_cap - 1); \
            for (size_t _d = 0; _d < _cap; _d++) { \
                if ((map)->hashes[_pos] == 0) \
                    break; \
                if (_HASHMAP_PROBE_DIST((map)->hashes[_pos], _pos, _cap) < _d) \
                    break; \
                if ((map)->hashes[_pos] == _h && (eq_fn)(&(map)->keys[_pos], &_k, sizeof(_k))) { \
                    (map)->hashes[_pos] = 0; \
                    (map)->count--; \
                    /* Backward-shift: move subsequent displaced entries back. */ \
                    size_t _empty = _pos; \
                    size_t _next = (_pos + 1) & (_cap - 1); \
                    while ((map)->hashes[_next] != 0 && _HASHMAP_PROBE_DIST((map)->hashes[_next], _next, _cap) > 0) { \
                        (map)->keys[_empty] = (map)->keys[_next]; \
                        (map)->values[_empty] = (map)->values[_next]; \
                        (map)->hashes[_empty] = (map)->hashes[_next]; \
                        (map)->hashes[_next] = 0; \
                        _empty = _next; \
                        _next = (_next + 1) & (_cap - 1); \
                    } \
                    _removed = true; \
                    break; \
                } \
                _pos = (_pos + 1) & (_cap - 1); \
            } \
        } \
        _removed; \
    })

// Iterate over all occupied entries. `idx` is a size_t loop variable.
// Access keys and values via (map)->keys[idx] and (map)->values[idx].
#define HASHMAP_FOREACH(map, idx) \
    for (size_t idx = 0; idx < (map)->capacity; idx++) \
        if ((map)->hashes[idx] != 0)
