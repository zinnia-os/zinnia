// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <stdint.h>

// From AMD64 SysV psABI.
enum {
    UNWIND_REG_RAX = 0,
    UNWIND_REG_RDX = 1,
    UNWIND_REG_RCX = 2,
    UNWIND_REG_RBX = 3,
    UNWIND_REG_RSI = 4,
    UNWIND_REG_RDI = 5,
    UNWIND_REG_RBP = 6,
    UNWIND_REG_RSP = 7,
    UNWIND_REG_R8 = 8,
    UNWIND_REG_R12 = 12,
    UNWIND_REG_R13 = 13,
    UNWIND_REG_R14 = 14,
    UNWIND_REG_R15 = 15,
    UNWIND_REG_RIP = 16,
};

#define UNWIND_NUM_REGS 17
#define UNWIND_REG_SP   UNWIND_REG_RSP
#define UNWIND_REG_FP   UNWIND_REG_RBP

// The frame record sits right below the CFA.
#define UNWIND_FP_KNOWS_SP 1

[[__force_inline]]
static inline void arch_unwind_capture(uintptr_t* regs, uintptr_t* pc, uint64_t* valid) {
    asm volatile(
        "lea rax, [rip + 1f]\n"
        "mov [%[pc]], rax\n"
        "mov [%[regs] + %c[rbx] * 8], rbx\n"
        "mov [%[regs] + %c[rbp] * 8], rbp\n"
        "mov [%[regs] + %c[rsp] * 8], rsp\n"
        "mov [%[regs] + %c[r12] * 8], r12\n"
        "mov [%[regs] + %c[r13] * 8], r13\n"
        "mov [%[regs] + %c[r14] * 8], r14\n"
        "mov [%[regs] + %c[r15] * 8], r15\n"
        "1:\n"
        :
        : [regs] "r"(regs),
          [pc] "r"(pc),
          [rbx] "i"(UNWIND_REG_RBX),
          [rbp] "i"(UNWIND_REG_RBP),
          [rsp] "i"(UNWIND_REG_RSP),
          [r12] "i"(UNWIND_REG_R12),
          [r13] "i"(UNWIND_REG_R13),
          [r14] "i"(UNWIND_REG_R14),
          [r15] "i"(UNWIND_REG_R15)
        : "rax", "memory"
    );

    // Mark all callee saved registers as valid.
    *valid = (1ull << UNWIND_REG_RBX) | (1ull << UNWIND_REG_RBP) | (1ull << UNWIND_REG_RSP) | (1ull << UNWIND_REG_R12) |
        (1ull << UNWIND_REG_R13) | (1ull << UNWIND_REG_R14) | (1ull << UNWIND_REG_R15);
}
