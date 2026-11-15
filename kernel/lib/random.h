// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/initgraph.h>
#include <stddef.h>
#include <stdint.h>

struct random_source {
    const char* name;
    uint8_t quality;

    bool (*get_u64)(struct random_source* source, uint64_t* out);
};

// Registers a source of randomness. Returns true if it is now the active source.
bool random_register(struct random_source* source);

void random_fill(void* buf, size_t len);

INIT_EXTERN(random_stage);
