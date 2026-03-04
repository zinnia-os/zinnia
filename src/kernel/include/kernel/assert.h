#pragma once

#include <common/compiler.h>
#include <kernel/panic.h>

#define ASSERT(expr, msg, ...) \
    ({ \
        if (__unlikely(!(expr))) { \
            panic( \
                "In function \"%s\" (%s:%u):\n" \
                "Assertion \"%s\" failed!\n" msg, \
                __FUNCTION__, \
                __FILE__, \
                __LINE__, \
                #expr, \
                ##__VA_ARGS__ \
            ); \
        } \
    })

#ifndef NDEBUG
#define DEBUG_ASSERT(...) ASSERT(__VA_ARGS__)
#else
#define DEBUG_ASSERT(...) ((void)0)
#endif