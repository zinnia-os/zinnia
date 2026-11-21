// SPDX-License-Identifier: BSD-3-Clause

// Code adapted from the RFC 1321 example

#include <lib/md5.h>
#include <string.h>

typedef uint32_t (*md5_func_t)(uint32_t x, uint32_t y, uint32_t z);

static constexpr uint32_t T[64] = {
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
};

static constexpr uint8_t S[4][4] = {{7, 12, 17, 22}, {5, 9, 14, 20}, {4, 11, 16, 23}, {6, 10, 15, 21}};

static uint32_t F(uint32_t x, uint32_t y, uint32_t z) {
    return (x & y) | (~x & z);
}

static uint32_t G(uint32_t x, uint32_t y, uint32_t z) {
    return (x & z) | (y & ~z);
}

static uint32_t H(uint32_t x, uint32_t y, uint32_t z) {
    return x ^ y ^ z;
}

static uint32_t I(uint32_t x, uint32_t y, uint32_t z) {
    return y ^ (x | ~z);
}

static void encode(uint8_t* output, const uint32_t* input, size_t len) {
    for (size_t j = 0; j < len; j++)
        output[j] = input[j / 4] >> (j % 4 * 8);
}

static void decode(uint32_t* output, const uint8_t* input, size_t len) {
    memset(output, 0, len);
    for (size_t j = 0; j < len; j++)
        output[j / 4] |= (uint32_t)input[j] << (j % 4 * 8);
}

static void md5_transform(uint32_t state[4], const uint8_t block[MD5_BLOCK_LEN]) {
    static const md5_func_t funcs[4] = {F, G, H, I};
    static constexpr uint8_t x_start[4] = {0, 1, 5, 0};
    static constexpr uint8_t x_step[4] = {1, 5, 3, 7};

    uint32_t a = state[0], b = state[1], c = state[2], d = state[3], x[16];
    decode(x, block, MD5_BLOCK_LEN);

    for (size_t i = 0; i < 64; i++) {
        const size_t r = i / 16;
        const uint32_t sum = a + funcs[r](b, c, d) + x[(x_start[r] + x_step[r] * i) % 16] + T[i];
        const uint8_t s = S[r][i % 4];
        a = d;
        d = c;
        c = b;
        b += (sum << s) | (sum >> (32 - s));
    }

    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    memset(x, 0, sizeof(x));
}

void md5_init(struct md5_ctx* ctx) {
    ctx->count = 0;
    ctx->state[0] = 0x67452301;
    ctx->state[1] = 0xefcdab89;
    ctx->state[2] = 0x98badcfe;
    ctx->state[3] = 0x10325476;
}

void md5_update(struct md5_ctx* ctx, const void* data, size_t len) {
    const uint8_t* input = data;
    const size_t index = ctx->count % MD5_BLOCK_LEN;
    const size_t part_len = MD5_BLOCK_LEN - index;
    ctx->count += len;

    if (len < part_len) {
        memcpy(&ctx->buf[index], input, len);
        return;
    }

    memcpy(&ctx->buf[index], input, part_len);
    md5_transform(ctx->state, ctx->buf);
    size_t i = part_len;
    for (; i + MD5_BLOCK_LEN <= len; i += MD5_BLOCK_LEN)
        md5_transform(ctx->state, &input[i]);
    memcpy(ctx->buf, &input[i], len - i);
}

void md5_final(struct md5_ctx* ctx, uint8_t digest[static MD5_DIGEST_LEN]) {
    static constexpr uint8_t padding[MD5_BLOCK_LEN] = {0x80};
    const uint64_t bits = ctx->count * 8;
    const uint32_t count[2] = {(uint32_t)bits, (uint32_t)(bits >> 32)};
    uint8_t bits_le[8];
    encode(bits_le, count, sizeof(bits_le));

    const size_t index = ctx->count % MD5_BLOCK_LEN;
    md5_update(ctx, padding, index < 56 ? 56 - index : 120 - index);
    md5_update(ctx, bits_le, sizeof(bits_le));

    encode(digest, ctx->state, MD5_DIGEST_LEN);
    memset(ctx, 0, sizeof(*ctx));
}
