#include <common/compiler.h>
#include <kernel/percpu.h>
#include <stddef.h>
#include <x86_64/defs.h>

void arch_syscall_handler() {}

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
        "mov rdi, rsp\n"              // Put the trap frame struct as first argument.
        "call arch_syscall_handler\n" // Call syscall handler
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
        ::"i"(offsetof(struct percpu, user_stack)),
        "i"(offsetof(struct percpu, kernel_stack)),
        "i"(offsetof(struct gdt, user_code64) | CPL_USER),
        "i"(offsetof(struct gdt, user_data) | CPL_USER)
    );
}
