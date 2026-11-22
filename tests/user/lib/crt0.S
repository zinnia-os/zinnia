// SPDX-License-Identifier: BSD-3-Clause

.intel_syntax noprefix
.text

.globl _start
.type _start, @function
_start:
    xor ebp, ebp
    mov rdi, rsp
    and rsp, -16
    call main
    mov edi, eax
    xor eax, eax
    syscall
    ud2
.size _start, . - _start

.section .note.GNU-stack, "", @progbits
