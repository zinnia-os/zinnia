// SPDX-License-Identifier: BSD-3-Clause

#include <arch/sched.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/gdt.h>
#include <lib/compiler.h>
#include <lib/percpu.h>
#include <syscalls/syscall.h>
#include <stddef.h>

// TODO: Refactor to only push callee-saved registers.

[[__naked]]
void arch_syscall_stub() {
    asm volatile(
        "    swapgs\n"
        "    mov gs:%c[user_stack], rsp\n"
        "    mov rsp, gs:%c[kernel_stack]\n"
        "    cld\n"
        "1:\n"
        "    push %c[user_data]\n" // ss
        "    push gs:%c[user_stack]\n"
        "    push r11\n"             // rflags
        "    push %c[user_code64]\n" // cs
        "    push rcx\n"             // rip
        "    push 0x00\n"            // arch_context::error
        "    push 0x00\n"            // arch_context::isr
        "    push rax\n"
        "    push rbx\n"
        "    push rcx\n"
        "    push rdx\n"
        "    push rbp\n"
        "    push rdi\n"
        "    push rsi\n"
        "    push r8\n"
        "    push r9\n"
        "    push r10\n"
        "    push r11\n"
        "    push r12\n"
        "    push r13\n"
        "    push r14\n"
        "    push r15\n"
        "    xor rbp, rbp\n"
        "    mov rdi, rsp\n" // SysV ABI first argument.
        "    sti\n"
        "    call syscall_dispatch\n" // Call syscall handler.
        "    cli\n"
        "    pop r15\n"
        "    pop r14\n"
        "    pop r13\n"
        "    pop r12\n"
        "    pop r11\n"
        "    pop r10\n"
        "    pop r9\n"
        "    pop r8\n"
        "    pop rsi\n"
        "    pop rdi\n"
        "    pop rbp\n"
        "    pop rdx\n"
        "    pop rcx\n"
        "    pop rbx\n"
        "    pop rax\n"
        "    add rsp, 0x10\n" // Skip .error and .isr fields
        "    pop rcx\n"       // rip
        "    add rsp, 0x08\n" // Skip cs
        "    pop r11\n"       // rflags
        "    pop rsp\n"       // rsp
        "    swapgs\n"
        "    sysretq\n"
        :
        : [user_stack] "i"(offsetof(struct percpu, user_stack)),
          [kernel_stack] "i"(offsetof(struct percpu, kernel_stack)),
          [user_code64] "i"(offsetof(struct gdt, user_code64) | CPL_USER),
          [user_data] "i"(offsetof(struct gdt, user_data) | CPL_USER)
    );
}
