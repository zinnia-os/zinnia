// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>

static inline size_t arch_page_size() {
    return 1 << 12;
}

static inline uintptr_t arch_mem_hhdm_addr() {
    return 0xFFFF'8000'0000'0000;
}

static inline uintptr_t arch_mem_pfndb_addr() {
    return 0xFFFF'A000'0000'0000;
}

static inline uintptr_t arch_mem_mapping_addr() {
    return 0xFFFF'C000'0000'0000;
}

static inline bool arch_mem_addr_is_user(uintptr_t addr) {
    return addr < 0x0000'7FFF'FFFF'F000;
}
