// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h> // IWYU pragma: keep

#if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
#define cpu_to_le16(x) ((uint16_t)(x))
#define cpu_to_le32(x) ((uint32_t)(x))
#define cpu_to_le64(x) ((uint64_t)(x))
#define cpu_to_be16(x) __builtin_bswap16(x)
#define cpu_to_be32(x) __builtin_bswap32(x)
#define cpu_to_be64(x) __builtin_bswap64(x)
#else
#define cpu_to_le16(x) __builtin_bswap16(x)
#define cpu_to_le32(x) __builtin_bswap32(x)
#define cpu_to_le64(x) __builtin_bswap64(x)
#define cpu_to_be16(x) ((uint16_t)(x))
#define cpu_to_be32(x) ((uint32_t)(x))
#define cpu_to_be64(x) ((uint64_t)(x))
#endif

#define le16_to_cpu(x) cpu_to_le16(x)
#define le32_to_cpu(x) cpu_to_le32(x)
#define le64_to_cpu(x) cpu_to_le64(x)
#define be16_to_cpu(x) cpu_to_be16(x)
#define be32_to_cpu(x) cpu_to_be32(x)
#define be64_to_cpu(x) cpu_to_be64(x)
