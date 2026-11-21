// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>

constexpr size_t MD5_DIGEST_LEN = 16;
constexpr size_t MD5_BLOCK_LEN = 64;

struct md5_ctx {
    uint32_t state[4];
    uint64_t count;
    uint8_t buf[MD5_BLOCK_LEN];
};

void md5_init(struct md5_ctx* ctx);
void md5_update(struct md5_ctx* ctx, const void* data, size_t len);
void md5_final(struct md5_ctx* ctx, uint8_t digest[static MD5_DIGEST_LEN]);
