// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

constexpr uint64_t ARCH_HUGE_PAGE_SIZE = 0x20'0000;

static inline uint64_t arch_hhdm_base() {
    return 0xffff'8000'0000'0000;
}
