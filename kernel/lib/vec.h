// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <mem/alloc.h>
#include <stddef.h> // IWYU pragma: keep
#include <string.h> // IWYU pragma: keep

#define vec(T) \
    struct { \
        T* data; \
        size_t length; \
        size_t capacity; \
    }

#define vec_init(vec) \
    do { \
        (vec)->data = nullptr; \
        (vec)->length = 0; \
        (vec)->capacity = 0; \
    } while (0)

#define vec_destroy(vec) \
    do { \
        mem_free((vec)->data, (vec)->capacity * sizeof(*(vec)->data)); \
        (vec)->data = nullptr; \
        (vec)->length = 0; \
        (vec)->capacity = 0; \
    } while (0)

#define vec_length(vec)      ((vec)->length)
#define vec_capacity(vec)    ((vec)->capacity)
#define vec_is_empty(vec)    ((vec)->length == 0)
#define vec_elem_get(vec, i) ((vec)->data[i])

// Internal: grow to at least `needed` capacity.
#define _vec_grow(vec, needed) \
    ({ \
        errno_t _grow_status = 0; \
        size_t _new_cap = (vec)->capacity == 0 ? 8 : (vec)->capacity; \
        while (_new_cap < (needed)) \
            _new_cap *= 2; \
        typeof((vec)->data) _new = mem_alloc(_new_cap * sizeof(*(vec)->data), ALLOC_NOZERO); \
        if (_new == nullptr) { \
            _grow_status = ENOMEM; \
        } else { \
            if ((vec)->length > 0) \
                memcpy(_new, (vec)->data, (vec)->length * sizeof(*(vec)->data)); \
            mem_free((vec)->data, (vec)->capacity * sizeof(*(vec)->data)); \
            (vec)->data = _new; \
            (vec)->capacity = _new_cap; \
        } \
        _grow_status; \
    })

// Append an element. Returns 0 or ENOMEM.
#define vec_push(vec, val) \
    ({ \
        errno_t _status = 0; \
        if ((vec)->length >= (vec)->capacity) \
            _status = _vec_grow(vec, (vec)->length + 1); \
        if (_status == 0) \
            (vec)->data[(vec)->length++] = (val); \
        _status; \
    })

// Remove and return the last element. Undefined if empty.
#define vec_pop(vec) ((vec)->data[--(vec)->length])

// Insert at index, shifting subsequent elements right. Returns 0 or ENOMEM.
#define vec_insert(vec, i, val) \
    ({ \
        errno_t _status = 0; \
        size_t _idx = (i); \
        if ((vec)->length >= (vec)->capacity) \
            _status = _vec_grow(vec, (vec)->length + 1); \
        if (_status == 0) { \
            memmove(&(vec)->data[_idx + 1], &(vec)->data[_idx], ((vec)->length - _idx) * sizeof(*(vec)->data)); \
            (vec)->data[_idx] = (val); \
            (vec)->length++; \
        } \
        _status; \
    })

// Remove element at index, shifting subsequent elements left.
#define vec_remove(vec, i) \
    do { \
        size_t _idx = (i); \
        (vec)->length--; \
        if (_idx < (vec)->length) \
            memmove(&(vec)->data[_idx], &(vec)->data[_idx + 1], ((vec)->length - _idx) * sizeof(*(vec)->data)); \
    } while (0)

// Remove element at index by swapping with the last element (O(1), unordered).
#define vec_swap_remove(vec, i) \
    do { \
        size_t _idx = (i); \
        (vec)->length--; \
        if (_idx < (vec)->length) \
            (vec)->data[_idx] = (vec)->data[(vec)->length]; \
    } while (0)

// Clear all elements without freeing memory.
#define vec_clear(vec) ((vec)->length = 0)

#define vec_foreach(vec, idx) for (size_t idx = 0; idx < (vec)->length; idx++)
