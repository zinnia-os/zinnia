/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_AARCH64_UCONTEXT_H
#define __ZINNIA_AARCH64_UCONTEXT_H

#include <zinnia/compiler.h>
#include <zinnia/signal.h>
#include <zinnia/types.h>

#define __NGREG 34

#define __FPSIMD_MAGIC 0x46508001
#define __ESR_MAGIC    0x45535201
#define __EXTRA_MAGIC  0x45585401
#define __SVE_MAGIC    0x53564501

struct __aarch64_ctx {
    __zn_u32 magic;
    __zn_u32 size;
};

struct __fpsimd_context {
    struct __aarch64_ctx head;
    __zn_u32 fpsr;
    __zn_u32 fpcr;
    __uint128_t vregs[32];
};

struct __esr_context {
    struct __aarch64_ctx head;
    __zn_u64 esr;
};

struct __extra_context {
    struct __aarch64_ctx head;
    __zn_u64 datap;
    __zn_u32 size;
    __zn_u32 __reserved[3];
};

typedef struct __mcontext {
    __zn_u64 fault_address;
    __zn_u64 regs[31];
    __zn_u64 sp;
    __zn_u64 pc;
    __zn_u64 pstate;
    __zn_u8 __reserved[4096] __attribute__((__aligned__(16)));
} __mcontext_t;

typedef struct __ucontext {
    unsigned long uc_flags;
    struct __ucontext __user* uc_link;
    struct __stack uc_stack;
    __sigset_t uc_sigmask;
    __mcontext_t uc_mcontext;
} __ucontext_t;

#endif
