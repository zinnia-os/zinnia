// SPDX-License-Identifier: BSD-3-Clause

    .intel_syntax noprefix
    .text

    .globl _start
    .type _start, @function
_start:
    // Align the stack pointer
    push rbp
    mov rbp, rsp
    push rdi
    push rsi
    push rbx
    push r12
    and rsp, -16
    cld

    mov rbx, rcx
    mov r12, rdx

    // Clear .bss
    lea rdi, [rip + __bss_start]
    lea rcx, [rip + __bss_end]
    sub rcx, rdi
    xor eax, eax
    rep stosb

    // Self-relocate
    lea rdi, [rip + __image_base]
    lea rsi, [rip + _DYNAMIC]
    call efistub_relocate
    test rax, rax
    jnz 1f

    mov rdi, rbx
    mov rsi, r12
    call efi_main

1:
    lea rsp, [rbp - 32]
    pop r12
    pop rbx
    pop rsi
    pop rdi
    pop rbp
    ret
    .size _start, . - _start
