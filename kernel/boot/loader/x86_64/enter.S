// SPDX-License-Identifier: BSD-3-Clause

    .intel_syntax noprefix
    .text

    .globl arch_enter_kernel
    .type arch_enter_kernel, @function
    .balign 16
arch_enter_kernel:
    cli
    mov cr3, rdi
    jmp rsi
    .size arch_enter_kernel, . - arch_enter_kernel
