// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <stddef.h>
#include <stdint.h>

[[__pure]]
bool arch_mem_addr_is_user(uintptr_t addr);

[[__pure]]
size_t arch_mem_page_size();

[[__pure]]
size_t arch_mem_page_bits();

[[__pure]]
size_t arch_mem_level_bits();

[[__pure]]
size_t arch_mem_num_levels();

[[__pure]]
uintptr_t arch_mem_hhdm_addr();

[[__pure]]
uintptr_t arch_mem_pfndb_addr();

[[__pure]]
uintptr_t arch_mem_mapping_addr();
