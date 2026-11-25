// SPDX-License-Identifier: BSD-3-Clause

    .text

    .globl _start
    .type _start, @function
_start:
    addi sp, sp, -32
    sd ra, 24(sp)
    sd s0, 16(sp)
    sd s1, 8(sp)
    mv s0, a0
    mv s1, a1

    lla t0, __bss_start
    lla t1, __bss_end
1:
    bgeu t0, t1, 2f
    sb zero, 0(t0)
    addi t0, t0, 1
    j 1b
2:

    lla a0, __image_base
    lla a1, _DYNAMIC
    call efistub_relocate
    bnez a0, 3f

    mv a0, s0
    mv a1, s1
    call efi_main

3:
    ld ra, 24(sp)
    ld s0, 16(sp)
    ld s1, 8(sp)
    addi sp, sp, 32
    ret
    .size _start, . - _start
