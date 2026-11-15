/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_RISCV64_UCONTEXT_H
#define __ZINNIA_RISCV64_UCONTEXT_H

#include <zinnia/compiler.h>
#include <zinnia/signal.h>
#include <zinnia/types.h>

#define __REG_PC    0
#define __REG_RA    1
#define __REG_SP    2
#define __REG_TP    4
#define __REG_S0    8
#define __REG_S1    9
#define __REG_A0    10
#define __REG_S2    18
#define __REG_NARGS 8
#define __NGREG     32

struct __riscv_mc_f_ext_state {
    __zn_u32 f[32];
    __zn_u32 fcsr;
};

struct __riscv_mc_d_ext_state {
    __zn_u64 f[32];
    __zn_u32 fcsr;
};

struct __riscv_mc_q_ext_state {
    __zn_u64 f[64] __attribute__((__aligned__(16)));
    __zn_u32 fcsr;
    __zn_u32 __reserved[3];
};

union __riscv_mc_fp_state {
    struct __riscv_mc_f_ext_state __f;
    struct __riscv_mc_d_ext_state __d;
    struct __riscv_mc_q_ext_state __q;
};

typedef struct __mcontext {
    unsigned long gregs[__NGREG];
    union __riscv_mc_fp_state fpregs;
} __mcontext_t;

typedef struct __ucontext {
    unsigned long uc_flags;
    struct __ucontext __user* uc_link;
    struct __stack uc_stack;
    __sigset_t uc_sigmask;
    __mcontext_t uc_mcontext;
} __ucontext_t;

#endif
