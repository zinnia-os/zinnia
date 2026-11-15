// SPDX-License-Identifier: BSD-3-Clause

#include <arch/mem.h>
#include <stddef.h>
#include <stdint.h>

size_t arch_mem_page_size() {
    return 1 << 12;
}

size_t arch_mem_page_bits() {
    return 12;
}

size_t arch_mem_level_bits() {
    return 9;
}

size_t arch_mem_num_levels() {
    return 4;
}

uintptr_t arch_mem_hhdm_addr() {
    return 0xFFFF'8000'0000'0000;
}

uintptr_t arch_mem_pfndb_addr() {
    return 0xFFFF'A000'0000'0000;
}

uintptr_t arch_mem_mapping_addr() {
    return 0xFFFF'C000'0000'0000;
}

bool arch_mem_addr_is_user(uintptr_t addr) {
    return addr < 0x0000'7FFF'FFFF'F000;
}
