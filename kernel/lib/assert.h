// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/panic.h>

#define assert(expr, msg, ...) \
    ({ \
        if (__unlikely(!(expr))) { \
            panic("Assertion \"%s\" failed: \"" msg "\" @ %s:%u\n", #expr, ##__VA_ARGS__, __FILE__, __LINE__); \
        } \
    })

#ifndef NDEBUG
#define debug_assert(...) assert(__VA_ARGS__)
#else
#define debug_assert(...) ((void)0)
#endif
