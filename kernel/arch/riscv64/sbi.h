// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

constexpr uint64_t SBI_EXT_TIME = 0x5449'4D45;
constexpr uint64_t SBI_EXT_DBCN = 0x4442'434E;

uint64_t sbi_call(uint64_t eid, uint64_t fid, uint64_t a0);

static inline void sbi_set_timer(uint64_t stime) {
    sbi_call(SBI_EXT_TIME, 0, stime);
}
