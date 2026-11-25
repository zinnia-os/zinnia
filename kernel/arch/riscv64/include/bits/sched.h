// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <bits/types.h>
#include <stdint.h>
#include <zinnia/mcontext.h>

struct arch_task_context {
    uint64_t sp;
    bool has_fp;
    struct __mcontext_riscv64_d_state fp;
};
typedef struct arch_task_context arch_task_context_t;

struct arch_context {
    union {
        struct {
            uint64_t sepc, ra, sp, gp, tp;
            uint64_t t0, t1, t2;
            uint64_t s0, s1;
            uint64_t a0, a1, a2, a3, a4, a5, a6, a7;
            uint64_t s2, s3, s4, s5, s6, s7, s8, s9, s10, s11;
            uint64_t t3, t4, t5, t6;
        };
        uint64_t x[32];
    };
    uint64_t sstatus;
};
typedef struct arch_context arch_context_t;

#define ARCH_CTX_NUM  a7
#define ARCH_CTX_SP   sp
#define ARCH_CTX_A0   a0
#define ARCH_CTX_A1   a1
#define ARCH_CTX_A2   a2
#define ARCH_CTX_A3   a3
#define ARCH_CTX_A4   a4
#define ARCH_CTX_A5   a5
#define ARCH_CTX_RET0 a0
#define ARCH_CTX_RET1 a1

struct arch_syscall_restart {
    uint64_t a0;
    uint64_t a1;
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
