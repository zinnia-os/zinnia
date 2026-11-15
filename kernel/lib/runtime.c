// SPDX-License-Identifier: BSD-3-Clause

// Compiler runtime support.

#include <stdint.h>

uint64_t __udivdi3(uint64_t dividend, uint64_t divisor) {
    if (divisor == 0)
        return 0;

    if (dividend < divisor)
        return 0;

    unsigned shift = __builtin_clzll(divisor) - __builtin_clzll(dividend);

    divisor <<= shift;

    uint64_t quotient = 0;

    for (;;) {
        if (dividend >= divisor) {
            dividend -= divisor;
            quotient |= (uint64_t)1 << shift;
        }

        if (shift == 0)
            break;

        shift--;
        divisor >>= 1;
    }

    return quotient;
}

static unsigned clz128(__uint128_t value) {
    uint64_t high = (uint64_t)(value >> 64);

    if (high != 0)
        return (unsigned)__builtin_clzll(high);

    return 64 + (unsigned)__builtin_clzll((uint64_t)value);
}

__uint128_t __udivmodti4(__uint128_t dividend, __uint128_t divisor, __uint128_t* remainder) {
    if (divisor == 0) {
        if (remainder)
            *remainder = dividend;

        return 0;
    }

    if (dividend < divisor) {
        if (remainder)
            *remainder = dividend;

        return 0;
    }

    unsigned shift = clz128(divisor) - clz128(dividend);

    divisor <<= shift;

    __uint128_t quotient = 0;

    for (;;) {
        if (dividend >= divisor) {
            dividend -= divisor;
            quotient |= (__uint128_t)1 << shift;
        }

        if (shift == 0)
            break;

        shift--;
        divisor >>= 1;
    }

    if (remainder)
        *remainder = dividend;

    return quotient;
}

__uint128_t __udivti3(__uint128_t dividend, __uint128_t divisor) {
    return __udivmodti4(dividend, divisor, nullptr);
}

__uint128_t __umodti3(__uint128_t dividend, __uint128_t divisor) {
    __uint128_t remainder;

    __udivmodti4(dividend, divisor, &remainder);

    return remainder;
}
