// SPDX-License-Identifier: BSD-3-Clause

    .text

    .globl arch_enter_kernel
    .type arch_enter_kernel, @function
    .balign 16
arch_enter_kernel:
    csrci sstatus, 2
    srli a0, a0, 12
    lla t0, loader_pmap_levels
    lbu t0, 0(t0)
    addi t0, t0, 5
    slli t0, t0, 60
    or a0, a0, t0
    csrw satp, a0
    sfence.vma
    jr a1
    .size arch_enter_kernel, . - arch_enter_kernel
