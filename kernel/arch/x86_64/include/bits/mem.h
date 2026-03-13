#pragma once

#include <kernel/types.h>
#include <stddef.h>
#include <stdint.h>

static inline size_t arch_mem_page_size() {
    return 1 << 12;
}

static inline size_t arch_mem_page_bits() {
    return 12;
}

static inline size_t arch_mem_level_bits() {
    return 9;
}

static inline size_t arch_mem_num_levels() {
    return 4;
}

static inline uintptr_t arch_mem_hhdm_addr() {
    return 0xFFFF'8000'0000'0000;
}

static inline uintptr_t arch_vm_pfndb_addr() {
    return 0xFFFF'A000'0000'0000;
}

static inline uintptr_t arch_mem_mapping_addr() {
    return 0xFFFF'C000'0000'0000;
}
