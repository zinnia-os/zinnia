/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_AARCH64_MCONTEXT_H
#define __ZINNIA_AARCH64_MCONTEXT_H

#include <zinnia/types.h>

#define __MC_TAG_AARCH64_SVE 0x53564501

struct __mcontext_aarch64_fpsimd_state {
    __zn_u32 fpsr;
    __zn_u32 fpcr;
    __zn_u64 __pad;
    __uint128_t vregs[32];
};

struct __mcontext_aarch64_sve_state {
    __zn_u16 vl;
    __zn_u16 flags;
    __zn_u16 __reserved[6];
};

typedef struct __mcontext {
    __zn_u64 mc_x[31];
    __zn_u64 mc_sp;
    __zn_u64 mc_pc;
    __zn_u64 mc_pstate;
    __zn_u64 mc_tpidr;
    __zn_u64 mc_esr;
    __zn_u64 mc_far;
    __zn_u64 __pad;
    struct __mcontext_aarch64_fpsimd_state mc_fpregs;
} __mcontext_t;

#endif
