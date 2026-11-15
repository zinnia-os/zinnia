// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/types.h>
#include <stdint.h>

struct __arch_task_context {
    uint64_t rsp;
    uint8_t* fpu_region;
    phys_t fpu_phys;
    uint16_t ds, es, fs, gs;
    uint64_t fs_base, gs_base;
};
typedef struct __arch_task_context __arch_task_context_t;

struct __arch_context {
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
typedef struct __arch_context __arch_context_t;

#define __ARCH_CTX_NUM  rax
#define __ARCH_CTX_SP   rsp
#define __ARCH_CTX_A0   rdi
#define __ARCH_CTX_A1   rsi
#define __ARCH_CTX_A2   rdx
#define __ARCH_CTX_A3   r10
#define __ARCH_CTX_A4   r8
#define __ARCH_CTX_A5   r9
#define __ARCH_CTX_RET0 rax
#define __ARCH_CTX_RET1 rdx

struct __arch_syscall_restart {
    uint64_t rax;
    uint64_t rdx;
};
typedef struct __arch_syscall_restart __arch_syscall_restart_t;
