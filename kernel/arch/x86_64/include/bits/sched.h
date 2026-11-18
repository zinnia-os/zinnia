// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <bits/types.h>
#include <stdint.h>

struct arch_task_context {
    uint64_t rsp;
    uint8_t* fpu_region;
    phys_t fpu_phys;
    uint16_t ds, es, fs, gs;
    uint64_t fs_base, gs_base;
};
typedef struct arch_task_context arch_task_context_t;

struct arch_context {
    uint64_t r15;
    uint64_t r14;
    uint64_t r13;
    uint64_t r12;
    uint64_t r11;
    uint64_t r10;
    uint64_t r9;
    uint64_t r8;
    uint64_t rsi;
    uint64_t rdi;
    uint64_t rbp;
    uint64_t rdx;
    uint64_t rcx;
    uint64_t rbx;
    uint64_t rax;
    uint64_t isr;   // Pushed onto the stack by the interrupt handler stubs.
    uint64_t error; // Pushed onto the stack by the CPU if the interrupt has an error code.
    uint64_t rip;   // The rest is pushed onto the stack by the CPU during an interrupt.
    uint64_t cs;
    uint64_t rflags;
    uint64_t rsp;
    uint64_t ss;
};
typedef struct arch_context arch_context_t;

#define ARCH_CTX_NUM  rax
#define ARCH_CTX_SP   rsp
#define ARCH_CTX_A0   rdi
#define ARCH_CTX_A1   rsi
#define ARCH_CTX_A2   rdx
#define ARCH_CTX_A3   r10
#define ARCH_CTX_A4   r8
#define ARCH_CTX_A5   r9
#define ARCH_CTX_RET0 rax
#define ARCH_CTX_RET1 rdx

struct arch_syscall_restart {
    uint64_t rax;
    uint64_t rdx;
};
typedef struct arch_syscall_restart arch_syscall_restart_t;

struct percpu;
struct task;

void arch_sched_kick(struct percpu* cpu);
struct task* arch_sched_switch(struct task* from, struct task* to);
errno_t arch_task_init(arch_task_context_t* context, void* entry, uintptr_t arg0, uintptr_t stack_start, bool is_user);
void arch_task_fini(arch_task_context_t* context);
void arch_task_fork(arch_task_context_t* dst);
void arch_context_init_user(arch_context_t* ctx, uintptr_t ip, uintptr_t sp);
[[noreturn]]
void arch_sched_jump_to_context(arch_context_t* context);
[[noreturn]]
void arch_sched_jump_to_user(uintptr_t ip, uintptr_t sp, uintptr_t arg);
