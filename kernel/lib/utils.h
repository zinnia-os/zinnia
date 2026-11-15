// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#define DIV_CEIL(value, to)   (((value) + ((to) - 1)) / (to))
#define ROUND_DOWN(value, to) (((value) / (to)) * (to))
#define ROUND_UP(value, to)   (DIV_CEIL(value, to) * (to))

#define MIN(a, b) ((a) < (b) ? (a) : (b))
#define MAX(a, b) ((a) > (b) ? (a) : (b))

#define BIT_TEST(value, bit) (((value) & (1 << bit)) == (1 << bit))

#if defined(__has_builtin)
#if __has_builtin(__builtin_types_compatible_p)
// Get pointer to parent object given pointer to a struct or union member.
#define containerof(ptr, type, member) \
    ({ \
        static_assert( \
            __builtin_types_compatible_p(typeof(*(ptr)), typeof(((type*)0)->member)), \
            "Incompatible types for containerof" \
        ); \
        (type*)((size_t)(ptr) - offsetof(type, member)); \
    })
#endif
#endif
#ifndef containerof
// Get pointer to parent object given pointer to a struct or union member.
#define containerof(ptr, type, member) (type*)((size_t)(ptr) - offsetof(type, member))
#endif

#define CONCAT(A, B)  _CONCAT(A, B)
#define _CONCAT(A, B) A##B

// Creates a unique identifier for a compilation unit.
#define UNIQUE_IDENT(ident) CONCAT(__unique_##ident, __COUNTER__)

// Ensures that a type is properly defined at this point.
// This is used to ensure that e.g. architecture-specific structures are defined.
#define ASSERT_TYPE(type) static_assert(sizeof(type) == sizeof(type))
