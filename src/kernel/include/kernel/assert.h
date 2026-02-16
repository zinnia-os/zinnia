#pragma once

#include <common/compiler.h>
#include <kernel/panic.h>
#include <kernel/percpu.h>
#include <kernel/print.h>

#define ASSERT(expr, msg, ...) \
    ({ \
        if (__unlikely(!(expr))) { \
            struct task* current = percpu_get()->sched.current; \
            size_t tid = current ? current->id : 0; \
            kprintf( \
                "\e[31m" \
                "Task %zu on CPU %zu panicked!\n" \
                "In function \"%s\" (%s:%u):\n" \
                "Assertion \"%s\" failed: " msg "\n", \
                tid, \
                percpu_get()->id, \
                __FUNCTION__, \
                __FILE__, \
                __LINE__, \
                #expr, \
                ##__VA_ARGS__ \
            ); \
            panic(); \
        } \
    })
