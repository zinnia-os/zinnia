// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

enum kstat_kind {
    KSTAT_KIND_I8,
    KSTAT_KIND_I16,
    KSTAT_KIND_I32,
    KSTAT_KIND_I64,
    KSTAT_KIND_U8,
    KSTAT_KIND_U16,
    KSTAT_KIND_U32,
    KSTAT_KIND_U64,
};

struct kstat_value {
    char name[32];
    enum kstat_kind kind;
    union {
        int8_t value_i8;
        int16_t value_i16;
        int32_t value_i32;
        int64_t value_i64;

        uint8_t value_u8;
        uint16_t value_u16;
        uint32_t value_u32;
        uint64_t value_u64;
    };
};
