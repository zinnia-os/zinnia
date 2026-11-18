// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>

static inline void arch_cpu_pause() {
    asm volatile("pause");
}

constexpr size_t ARCH_CACHE_LINE_SIZE = 64;
