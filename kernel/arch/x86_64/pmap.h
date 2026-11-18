// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

enum : uint64_t {
    ARCH_FLAG_PRESENT = 1 << 0,
    ARCH_FLAG_READ_WRITE = 1 << 1,
    ARCH_FLAG_USER_MODE = 1 << 2,
    ARCH_FLAG_WRITE_THROUGH = 1 << 3,
    ARCH_FLAG_CACHE_DISABLE = 1 << 4,
    ARCH_FLAG_ACCESSED = 1 << 5,
    ARCH_FLAG_DIRTY = 1 << 6,
    ARCH_FLAG_SIZE = 1 << 7,
    ARCH_FLAG_GLOBAL = 1 << 8,
    ARCH_FLAG_AVAILABLE = 1 << 9,
    ARCH_FLAG_ATTRIBUTE_TABLE = 1 << 7,
    ARCH_FLAG_EXECUTE_DISABLE = 1LU << 63,

    ARCH_PTE_ADDR_MASK = 0x000F'FFFF'FFFF'F000,
};
