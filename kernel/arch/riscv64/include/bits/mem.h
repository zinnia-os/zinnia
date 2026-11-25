// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/riscv64/pmap.h>
#include <stddef.h>
#include <stdint.h>

extern uint8_t riscv_pmap_levels;

static inline size_t arch_page_size() {
    return 1 << 12;
}

static inline uintptr_t arch_mem_hhdm_addr() {
    return riscv_pmap_levels == 3 ? ARCH_SV39_HHDM : ARCH_SV48_HHDM;
}

static inline uintptr_t arch_mem_pfndb_addr() {
    return riscv_pmap_levels == 3 ? ARCH_SV39_PFNDB : ARCH_SV48_PFNDB;
}

static inline uintptr_t arch_mem_mapping_addr() {
    return riscv_pmap_levels == 3 ? ARCH_SV39_MAPPING : ARCH_SV48_MAPPING;
}

static inline bool arch_mem_addr_is_user(uintptr_t addr) {
    return addr < (riscv_pmap_levels == 3 ? ARCH_SV39_USER_END : ARCH_SV48_USER_END);
}
