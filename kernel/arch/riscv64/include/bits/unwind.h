// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <stdint.h>

enum {
    UNWIND_REG_ZERO = 0,
    UNWIND_REG_RA = 1,
    UNWIND_REG_SP = 2,
    UNWIND_REG_GP = 3,
    UNWIND_REG_TP = 4,
    UNWIND_REG_T0 = 5,
    UNWIND_REG_T1 = 6,
    UNWIND_REG_T2 = 7,
    UNWIND_REG_S0 = 8,
    UNWIND_REG_S1 = 9,
    UNWIND_REG_A0 = 10,
    UNWIND_REG_A1 = 11,
    UNWIND_REG_A2 = 12,
    UNWIND_REG_A3 = 13,
    UNWIND_REG_A4 = 14,
    UNWIND_REG_A5 = 15,
    UNWIND_REG_A6 = 16,
    UNWIND_REG_A7 = 17,
    UNWIND_REG_S2 = 18,
    UNWIND_REG_S3 = 19,
    UNWIND_REG_S4 = 20,
    UNWIND_REG_S5 = 21,
    UNWIND_REG_S6 = 22,
    UNWIND_REG_S7 = 23,
    UNWIND_REG_S8 = 24,
    UNWIND_REG_S9 = 25,
    UNWIND_REG_S10 = 26,
    UNWIND_REG_S11 = 27,
    UNWIND_REG_T3 = 28,
    UNWIND_REG_T4 = 29,
    UNWIND_REG_T5 = 30,
    UNWIND_REG_T6 = 31,
    UNWIND_REG_PC = 32,
};

#define UNWIND_NUM_REGS 33
#define UNWIND_REG_FP   UNWIND_REG_S0

#define UNWIND_FP_KNOWS_SP 0

[[__force_inline]]
static inline void arch_unwind_capture(uintptr_t* regs, uintptr_t* pc, uint64_t* valid) {
    asm volatile(
        "lla t0, 1f\n"
        "sd t0, 0(%[pc])\n"
        "sd sp, %c[sp] * 8(%[regs])\n"
        "sd s0, %c[s0] * 8(%[regs])\n"
        "sd s1, %c[s1] * 8(%[regs])\n"
        "sd s2, %c[s2] * 8(%[regs])\n"
        "sd s3, %c[s3] * 8(%[regs])\n"
        "sd s4, %c[s4] * 8(%[regs])\n"
        "sd s5, %c[s5] * 8(%[regs])\n"
        "sd s6, %c[s6] * 8(%[regs])\n"
        "sd s7, %c[s7] * 8(%[regs])\n"
        "sd s8, %c[s8] * 8(%[regs])\n"
        "sd s9, %c[s9] * 8(%[regs])\n"
        "sd s10, %c[s10] * 8(%[regs])\n"
        "sd s11, %c[s11] * 8(%[regs])\n"
        "1:\n"
        :
        : [regs] "r"(regs),
          [pc] "r"(pc),
          [sp] "i"(UNWIND_REG_SP),
          [s0] "i"(UNWIND_REG_S0),
          [s1] "i"(UNWIND_REG_S1),
          [s2] "i"(UNWIND_REG_S2),
          [s3] "i"(UNWIND_REG_S3),
          [s4] "i"(UNWIND_REG_S4),
          [s5] "i"(UNWIND_REG_S5),
          [s6] "i"(UNWIND_REG_S6),
          [s7] "i"(UNWIND_REG_S7),
          [s8] "i"(UNWIND_REG_S8),
          [s9] "i"(UNWIND_REG_S9),
          [s10] "i"(UNWIND_REG_S10),
          [s11] "i"(UNWIND_REG_S11)
        : "t0", "memory"
    );

    *valid = (1ull << UNWIND_REG_SP) | (1ull << UNWIND_REG_S0) | (1ull << UNWIND_REG_S1) | (1ull << UNWIND_REG_S2) |
        (1ull << UNWIND_REG_S3) | (1ull << UNWIND_REG_S4) | (1ull << UNWIND_REG_S5) | (1ull << UNWIND_REG_S6) |
        (1ull << UNWIND_REG_S7) | (1ull << UNWIND_REG_S8) | (1ull << UNWIND_REG_S9) | (1ull << UNWIND_REG_S10) |
        (1ull << UNWIND_REG_S11);
}
