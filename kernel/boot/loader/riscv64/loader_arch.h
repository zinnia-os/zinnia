// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/riscv64/pmap.h>
#include <stdint.h>

extern uint8_t loader_pmap_levels;

constexpr uint64_t ARCH_HUGE_PAGE_SIZE = 0x20'0000;

static inline uint64_t arch_hhdm_base() {
    return loader_pmap_levels == 3 ? ARCH_SV39_HHDM : ARCH_SV48_HHDM;
}
