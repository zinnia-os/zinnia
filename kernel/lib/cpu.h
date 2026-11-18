// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <bits/cpu.h>
#include <stddef.h>

constexpr size_t CPU_CACHE_LINE_SIZE = ARCH_CACHE_LINE_SIZE;

static inline void cpu_pause() {
    arch_cpu_pause();
}
