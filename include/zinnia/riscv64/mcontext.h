/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_RISCV64_MCONTEXT_H
#define __ZINNIA_RISCV64_MCONTEXT_H

#include <zinnia/types.h>

#define __MC_TAG_RISCV64_V 0x52564656

struct __mcontext_riscv64_f_state {
    __zn_u32 f[32];
    __zn_u32 fcsr;
};

struct __mcontext_riscv64_d_state {
    __zn_u64 f[32];
    __zn_u32 fcsr;
    __zn_u32 __reserved[3];
};

struct __mcontext_riscv64_q_state {
    __zn_u64 f[64] __attribute__((__aligned__(16)));
    __zn_u32 fcsr;
    __zn_u32 __reserved[3];
};

union __mcontext_riscv64_fp_state {
    struct __mcontext_riscv64_f_state f;
    struct __mcontext_riscv64_d_state d;
    struct __mcontext_riscv64_q_state q;
};

typedef struct __mcontext {
    __zn_u64 mc_x[32];
    __zn_u64 mc_scause;
    __zn_u64 mc_stval;
    union __mcontext_riscv64_fp_state mc_fpregs;
} __mcontext_t;

#endif
