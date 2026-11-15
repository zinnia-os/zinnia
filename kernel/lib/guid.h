// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <stddef.h>
#include <stdint.h>

struct [[__packed]] guid {
    uint32_t time_low;
    uint16_t time_mid;
    uint16_t time_hi_version;
    uint8_t clock_seq[2];
    uint8_t node[6];
};
static_assert(sizeof(struct guid) == 16);

constexpr size_t GUID_STR_LEN = 37;

bool guid_is_zero(const struct guid* guid);
bool guid_equal(const struct guid* a, const struct guid* b);
void guid_format(const struct guid* guid, char out[static GUID_STR_LEN]);
