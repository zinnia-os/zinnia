#include <common/compiler.h>
#include <kernel/percpu.h>
#include <kernel/syscalls.h>
#include <bits/sched.h>
#include <stddef.h>
#include <x86_64/defs.h>

#define ASM_REG_NUM "rax"
#define ASM_REG_RET "rax"
#define ASM_REG_A0  "rdi"
#define ASM_REG_A1  "rsi"
#define ASM_REG_A2  "rdx"
#define ASM_REG_A3  "r9"
#define ASM_REG_A4  "r8"
#define ASM_REG_A5  "r10"
#define ASM_REG_A6  "r12"
#define ASM_REG_A7  "r13"
#define ASM_SYSCALL "syscall"

void arch_syscall_handler(struct arch_context* ctx) {
    ctx->rax = syscall_dispatch(ctx->rax, ctx->rdi, ctx->rsi, ctx->rdx, ctx->r9, ctx->r8, ctx->r10, ctx->r12, ctx->r13);
}

[[__naked]]
void arch_syscall_stub() {
    asm volatile(
        "swapgs\n"
        "mov gs:%c0, rsp\n"
        "mov rsp, gs:%c1\n"
        "cld\n"
        // We're pretending to be an interrupt, so fill the bottom fields of `Context`.
        "push %c3\n" // SS and CS are not changed during SYSCALL. Use `Gdt::user_data | CPL_USER`.
        "push gs:%c0\n"
        "push r11\n"  // RFLAGS is moved into r11 by the CPU.
        "push %c2\n"  // Same as SS. Use `Gdt::user_code64 | CPL_USER`
        "push rcx\n"  // RIP is moved into rcx by the CPU.
        "push 0x00\n" // Context::error field
        "push 0x00\n" // Context::isr field
        "push rax\n"
        "push rbx\n"
        "push rcx\n"
        "push rdx\n"
        "push rbp\n"
        "push rdi\n"
        "push rsi\n"
        "push r8\n"
        "push r9\n"
        "push r10\n"
        "push r11\n"
        "push r12\n"
        "push r13\n"
        "push r14\n"
        "push r15\n"
        "xor rbp, rbp\n"
        "mov rdi, rsp\n" // Put the trap frame struct as first argument.
        "call %c4\n"     // Call syscall handler
        "cli\n"
        "pop r15\n"
        "pop r14\n"
        "pop r13\n"
        "pop r12\n"
        "pop r11\n"
        "pop r10\n"
        "pop r9\n"
        "pop r8\n"
        "pop rsi\n"
        "pop rdi\n"
        "pop rbp\n"
        "pop rdx\n"
        "pop rcx\n"
        "pop rbx\n"
        "pop rax\n"
        "add rsp, 0x10\n"   // Skip .error and .isr fields (2 * sizeof(u64))
        "mov rsp, gs:%c0\n" // Load user stack from `Cpu.user_stack`.
        "swapgs\n"
        "sysretq\n" // Return to user mode.
        :
        : "i"(offsetof(struct percpu, user_stack)),
          "i"(offsetof(struct percpu, kernel_stack)),
          "i"(offsetof(struct gdt, user_code64) | CPL_USER),
          "i"(offsetof(struct gdt, user_data) | CPL_USER),
          "i"(arch_syscall_handler)
    );
}
