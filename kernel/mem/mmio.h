// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/types.h>
#include <mem/pmap.h>
#include <stddef.h>
#include <stdint.h>

errno_t mmio_map(phys_t base, size_t size, enum cache_mode cache, void** out);

static inline uint8_t mmio_read8(void* ptr) {
    return *((volatile uint8_t*)ptr);
}

static inline uint16_t mmio_read16(void* ptr) {
    return *((volatile uint16_t*)ptr);
}

static inline uint32_t mmio_read32(void* ptr) {
    return *((volatile uint32_t*)ptr);
}

static inline uint64_t mmio_read64(void* ptr) {
    return *((volatile uint64_t*)ptr);
}

static inline void mmio_write8(void* ptr, uint8_t value) {
    *((volatile uint8_t*)ptr) = value;
}

static inline void mmio_write16(void* ptr, uint16_t value) {
    *((volatile uint16_t*)ptr) = value;
}

static inline void mmio_write32(void* ptr, uint32_t value) {
    *((volatile uint32_t*)ptr) = value;
}

static inline void mmio_write64(void* ptr, uint64_t value) {
    *((volatile uint64_t*)ptr) = value;
}

enum mmio_endian : uint8_t {
    MMIO_NATIVE = 0,
#if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
    MMIO_LE = 0,
    MMIO_BE = 1,
#else
    MMIO_BE = 0,
    MMIO_LE = 1,
#endif
};

typedef struct {
    size_t offset;
    enum mmio_endian endian;
} mmio_reg8;

typedef struct {
    size_t offset;
    enum mmio_endian endian;
} mmio_reg16;

typedef struct {
    size_t offset;
    enum mmio_endian endian;
} mmio_reg32;

typedef struct {
    size_t offset;
    enum mmio_endian endian;
} mmio_reg64;

typedef struct {
    mmio_reg8 reg;
    uint8_t shift;
    uint8_t width;
} mmio_field8;

typedef struct {
    mmio_reg16 reg;
    uint8_t shift;
    uint8_t width;
} mmio_field16;

typedef struct {
    mmio_reg32 reg;
    uint8_t shift;
    uint8_t width;
} mmio_field32;

typedef struct {
    mmio_reg64 reg;
    uint8_t shift;
    uint8_t width;
} mmio_field64;

#define MMIO_REG_ZERO(reg) \
    _Generic((reg), mmio_reg8: (uint8_t)0, mmio_reg16: (uint16_t)0, mmio_reg32: (uint32_t)0, mmio_reg64: (uint64_t)0)

#define MMIO_REG_TYPE(reg)     typeof(MMIO_REG_ZERO(reg))
#define MMIO_FIELD_TYPE(field) MMIO_REG_TYPE((field).reg)

#define mmio_bswap(value) \
    _Generic( \
        (value), \
        uint8_t: (value), \
        uint16_t: __builtin_bswap16(value), \
        uint32_t: __builtin_bswap32(value), \
        uint64_t: __builtin_bswap64(value) \
    )

#define mmio_width_mask(width) ((width) >= 64 ? UINT64_MAX : ((uint64_t)1 << (width)) - 1)

// Reads an MMIO register.
#define mmio_reg_read(base, reg) \
    ({ \
        auto __mmio_rr = (reg); \
        typedef MMIO_REG_TYPE(__mmio_rr) __mmio_rt; \
        __mmio_rt __mmio_rv = *(volatile __mmio_rt*)((uint8_t*)(base) + __mmio_rr.offset); \
        (__mmio_rt)(__mmio_rr.endian ? mmio_bswap(__mmio_rv) : __mmio_rv); \
    })

// Writes a value to an MMIO register.
#define mmio_reg_write(base, reg, value) \
    ({ \
        auto __mmio_rw = (reg); \
        static_assert( \
            sizeof(value) <= sizeof(MMIO_REG_TYPE(reg)), \
            "High bits of the value might get truncated. Explicitly cast if this is intended." \
        ); \
        typedef MMIO_REG_TYPE(__mmio_rw) __mmio_wt; \
        __mmio_wt __mmio_wv = (value); \
        *(volatile __mmio_wt*)((uint8_t*)(base) + __mmio_rw.offset) = \
            __mmio_rw.endian ? mmio_bswap(__mmio_wv) : __mmio_wv; \
    })

// Shifts and masks the bits of `value` to the position it needs to be stored in the register.
#define mmio_field(field, value) ((MMIO_FIELD_TYPE(field))(((value) & mmio_width_mask((field).width)) << (field).shift))

// Returns the contents of a `field` from a `value`.
#define mmio_field_get(field, value) \
    ((MMIO_FIELD_TYPE(field))(((value) >> (field).shift) & mmio_width_mask((field).width)))
