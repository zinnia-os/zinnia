// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <stdint.h>

#if defined(__x86_64__)
#include <arch/x86_64/bits/sched.h>
#elif defined(__aarch64__)
#include <arch/aarch64/bits/sched.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <arch/riscv64/bits/sched.h>
#elif defined(__loongarch__) && __loongarch_grlen == 64
#include <arch/loongarch64/bits/sched.h>
#else
#error "Unsupported architecture"
#endif

typedef __arch_context_t arch_context_t;
typedef __arch_task_context_t arch_task_context_t;
typedef __arch_syscall_restart_t arch_syscall_restart_t;

#define ARCH_CTX_NUM  __ARCH_CTX_NUM
#define ARCH_CTX_SP   __ARCH_CTX_SP
#define ARCH_CTX_A0   __ARCH_CTX_A0
#define ARCH_CTX_A1   __ARCH_CTX_A1
#define ARCH_CTX_A2   __ARCH_CTX_A2
#define ARCH_CTX_A3   __ARCH_CTX_A3
#define ARCH_CTX_A4   __ARCH_CTX_A4
#define ARCH_CTX_A5   __ARCH_CTX_A5
#define ARCH_CTX_RET0 __ARCH_CTX_RET0
#define ARCH_CTX_RET1 __ARCH_CTX_RET1

struct percpu;
struct task;

void arch_sched_preempt_disable();
bool arch_sched_preempt_enable();
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
