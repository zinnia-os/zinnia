// SPDX-License-Identifier: BSD-3-Clause

#include <arch/usercopy.h>
#include <arch/x86_64/percpu.h>
#include <lib/compiler.h>
#include <lib/percpu.h>

void arch_usercopy_allow(bool state) {
    if (!percpu_ref(arch_data)->can_smap)
        return;

    if (state) {
        asm volatile("stac" ::: "memory");
    } else {
        asm volatile("clac" ::: "memory");
    }
}

extern void x86_64_read_start();
extern void x86_64_read_end();
extern void x86_64_read_fault();

static const struct usercopy_region read_region = {
    .start_ip = x86_64_read_start,
    .end_ip = x86_64_read_end,
    .fault_ip = x86_64_read_fault,
};

[[__naked]]
bool arch_usercopy_read(void* dst, const __user void* src, size_t len, struct usercopy_region** region) {
    asm volatile(
        // Setup regs for `rep movsb`.
        "    xchg rcx, rdx\n"
        "    lea rax, [rip + %0]\n"
        "    xchg [rdx], rax\n"

        ".global x86_64_read_start\n"
        "    x86_64_read_start:\n"
        "    rep movsb\n"

        ".global x86_64_read_end\n"
        "x86_64_read_end:\n"
        "    xor rax, rax\n"
        "    xchg [rdx], rax\n"
        "    mov rax, 1\n"
        "    ret\n"

        ".global x86_64_read_fault\n"
        "x86_64_read_fault:\n"
        "    xor rax, rax\n"
        "    mov [rdx], rax\n"
        "    ret\n"
        :
        : "i"(&read_region)
        : "memory"
    );
}

extern void x86_64_write_start();
extern void x86_64_write_end();
extern void x86_64_write_fault();

static const struct usercopy_region write_region = {
    .start_ip = x86_64_write_start,
    .end_ip = x86_64_write_end,
    .fault_ip = x86_64_write_fault,
};

[[__naked]]
bool arch_usercopy_write(__user void* dst, const void* src, size_t len, struct usercopy_region** region) {
    asm volatile(
        // Setup regs for `rep movsb`.
        "    xchg rcx, rdx\n"
        "    lea rax, [rip + %0]\n"
        "    mov [rdx], rax\n"

        ".global x86_64_write_start\n"
        "    x86_64_write_start:\n"
        "    rep movsb\n"

        ".global x86_64_write_end\n"
        "    x86_64_write_end:\n"
        "    xor rax, rax\n"
        "    xchg [rdx], rax\n"
        "    mov rax, 1\n"
        "    ret\n"

        ".global x86_64_write_fault\n"
        "x86_64_write_fault:\n"
        "    xor rax, rax\n"
        "    mov [rdx], rax\n"
        "    ret\n"
        :
        : "i"(&write_region)
        : "memory"
    );
}

extern void x86_64_strlen_start();
extern void x86_64_strlen_end();
extern void x86_64_strlen_fault();

static const struct usercopy_region strlen_region = {
    .start_ip = x86_64_strlen_start,
    .end_ip = x86_64_strlen_end,
    .fault_ip = x86_64_strlen_fault,
};

[[__naked]]
bool arch_usercopy_strlen(const __user char* str, size_t max, size_t* len, struct usercopy_region** region) {
    asm volatile(
        "    lea rax, [rip + %0]\n"
        "    xchg [rcx], rax\n"

        ".global x86_64_strlen_start\n"
        "    x86_64_strlen_start:\n"
        "    xor r8, r8\n"
        ".Lloop:\n"
        "    cmp r8, rsi\n"
        "    je .Lleave\n"
        "    cmp byte ptr [rdi + r8], 0\n"
        "    je .Lleave\n"
        "    inc r8\n"
        "    jmp .Lloop\n"
        ".Lleave:\n"
        "    mov [rdx], r8\n"

        ".global x86_64_strlen_end\n"
        "x86_64_strlen_end:\n"
        "    xor rax, rax\n"
        "    xchg [rcx], rax\n"
        "    mov rax, 1\n"
        "    ret\n"

        ".global x86_64_strlen_fault\n"
        "x86_64_strlen_fault:\n"
        "    xor rax, rax\n"
        "    mov [rcx], rax\n"
        "    ret\n"
        :
        : "i"(&strlen_region)
        : "memory"
    );
}
