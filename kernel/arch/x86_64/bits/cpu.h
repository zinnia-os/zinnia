// SPDX-License-Identifier: BSD-3-Clause

#pragma once

static inline void arch_cpu_pause() {
    asm volatile("pause");
}

#define __ARCH_CACHE_LINE_SIZE 64
