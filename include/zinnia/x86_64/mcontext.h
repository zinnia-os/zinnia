/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_X86_64_MCONTEXT_H
#define __ZINNIA_X86_64_MCONTEXT_H

#include <zinnia/types.h>

#define __MC_TAG_X86_64_XSAVE       0x58534156
#define __MC_TAG_X86_64_XSAVE_ALIGN 64

struct __mcontext_x86_64_fxsave_state {
    __zn_u16 cwd;
    __zn_u16 swd;
    __zn_u8 ftw;
    __zn_u8 __reserved0;
    __zn_u16 fop;
    __zn_u64 rip;
    __zn_u64 rdp;
    __zn_u32 mxcsr;
    __zn_u32 mxcsr_mask;
    __zn_u8 st[8][16];
    __zn_u8 xmm[16][16];
    __zn_u8 __reserved1[96];
} __attribute__((__aligned__(16)));

typedef struct __mcontext {
    __zn_u64 mc_r15;
    __zn_u64 mc_r14;
    __zn_u64 mc_r13;
    __zn_u64 mc_r12;
    __zn_u64 mc_r11;
    __zn_u64 mc_r10;
    __zn_u64 mc_r9;
    __zn_u64 mc_r8;
    __zn_u64 mc_rsi;
    __zn_u64 mc_rdi;
    __zn_u64 mc_rbp;
    __zn_u64 mc_rdx;
    __zn_u64 mc_rcx;
    __zn_u64 mc_rbx;
    __zn_u64 mc_rax;
    __zn_u64 mc_rip;
    __zn_u64 mc_rflags;
    __zn_u64 mc_rsp;
    __zn_u64 mc_fsbase;
    __zn_u64 mc_gsbase;
    __zn_u64 mc_trapno;
    __zn_u64 mc_err;
    __zn_u64 mc_cr2;
    __zn_u64 __pad;
    struct __mcontext_x86_64_fxsave_state mc_fpregs;
} __mcontext_t;

#endif
