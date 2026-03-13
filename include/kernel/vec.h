#pragma once

#include <kernel/alloc.h>
#include <uapi/errno.h>
#include <stddef.h>
#include <string.h>

#define VEC(T) \
    struct { \
        T* data; \
        size_t length; \
        size_t capacity; \
    }

#define VEC_INIT(vec) \
    do { \
        (vec)->data = nullptr; \
        (vec)->length = 0; \
        (vec)->capacity = 0; \
    } while (0)

#define VEC_DESTROY(vec) \
    do { \
        mem_free((vec)->data); \
        (vec)->data = nullptr; \
        (vec)->length = 0; \
        (vec)->capacity = 0; \
    } while (0)

#define VEC_LENGTH(vec)   ((vec)->length)
#define VEC_CAPACITY(vec) ((vec)->capacity)
#define VEC_EMPTY(vec)    ((vec)->length == 0)
#define VEC_AT(vec, i)    ((vec)->data[i])

// Internal: grow to at least `needed` capacity.
#define _VEC_GROW(vec, needed) \
    do { \
        size_t _new_cap = (vec)->capacity == 0 ? 8 : (vec)->capacity; \
        while (_new_cap < (needed)) \
            _new_cap *= 2; \
        typeof((vec)->data) _new = mem_alloc(_new_cap * sizeof(*(vec)->data), ALLOC_NOZERO); \
        if (_new != nullptr && (vec)->length > 0) \
            memcpy(_new, (vec)->data, (vec)->length * sizeof(*(vec)->data)); \
        mem_free((vec)->data); \
        (vec)->data = _new; \
        (vec)->capacity = _new_cap; \
    } while (0)

// Append an element. Returns 0 or ENOMEM.
#define VEC_PUSH(vec, val) \
    ({ \
        errno_t _status = 0; \
        if ((vec)->length >= (vec)->capacity) { \
            _VEC_GROW(vec, (vec)->length + 1); \
            if ((vec)->data == nullptr) \
                _status = ENOMEM; \
        } \
        if (_status == 0) \
            (vec)->data[(vec)->length++] = (val); \
        _status; \
    })

// Remove and return the last element. Undefined if empty.
#define VEC_POP(vec) ((vec)->data[--(vec)->length])

// Insert at index, shifting subsequent elements right. Returns 0 or ENOMEM.
#define VEC_INSERT(vec, i, val) \
    ({ \
        errno_t _status = 0; \
        size_t _idx = (i); \
        if ((vec)->length >= (vec)->capacity) { \
            _VEC_GROW(vec, (vec)->length + 1); \
            if ((vec)->data == nullptr) \
                _status = ENOMEM; \
        } \
        if (_status == 0) { \
            memmove(&(vec)->data[_idx + 1], &(vec)->data[_idx], ((vec)->length - _idx) * sizeof(*(vec)->data)); \
            (vec)->data[_idx] = (val); \
            (vec)->length++; \
        } \
        _status; \
    })

// Remove element at index, shifting subsequent elements left.
#define VEC_REMOVE(vec, i) \
    do { \
        size_t _idx = (i); \
        (vec)->length--; \
        if (_idx < (vec)->length) \
            memmove(&(vec)->data[_idx], &(vec)->data[_idx + 1], ((vec)->length - _idx) * sizeof(*(vec)->data)); \
    } while (0)

// Remove element at index by swapping with the last element (O(1), unordered).
#define VEC_SWAP_REMOVE(vec, i) \
    do { \
        size_t _idx = (i); \
        (vec)->length--; \
        if (_idx < (vec)->length) \
            (vec)->data[_idx] = (vec)->data[(vec)->length]; \
    } while (0)

// Clear all elements without freeing memory.
#define VEC_CLEAR(vec) ((vec)->length = 0)

#define VEC_FOREACH(vec, idx) for (size_t idx = 0; idx < (vec)->length; idx++)
