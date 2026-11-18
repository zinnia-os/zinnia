// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <bits/mem.h>
#include <stdint.h>

static inline bool mem_addr_is_user(uintptr_t addr) {
    return arch_mem_addr_is_user(addr);
}

static inline uintptr_t mem_hhdm_addr() {
    return arch_mem_hhdm_addr();
}

static inline uintptr_t mem_pfndb_addr() {
    return arch_mem_pfndb_addr();
}

static inline uintptr_t mem_mapping_addr() {
    return arch_mem_mapping_addr();
}
