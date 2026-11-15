/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_X86_64_UCONTEXT_H
#define __ZINNIA_X86_64_UCONTEXT_H

#include <zinnia/compiler.h>
#include <zinnia/signal.h>
#include <zinnia/types.h>

#define __REG_R8      0
#define __REG_R9      1
#define __REG_R10     2
#define __REG_R11     3
#define __REG_R12     4
#define __REG_R13     5
#define __REG_R14     6
#define __REG_R15     7
#define __REG_RDI     8
#define __REG_RSI     9
#define __REG_RBP     10
#define __REG_RBX     11
#define __REG_RDX     12
#define __REG_RAX     13
#define __REG_RCX     14
#define __REG_RSP     15
#define __REG_RIP     16
#define __REG_EFL     17
#define __REG_CSGSFS  18
#define __REG_ERR     19
#define __REG_TRAPNO  20
#define __REG_OLDMASK 21
#define __REG_CR2     22
#define __NGREG       23

struct __fpxreg {
    unsigned short significand[4];
    unsigned short exponent;
    unsigned short __padding[3];
};

struct __xmmreg {
    __zn_u32 element[4];
};

struct __fpstate {
    __zn_u16 cwd;
    __zn_u16 swd;
    __zn_u16 ftw;
    __zn_u16 fop;
    __zn_u64 rip;
    __zn_u64 rdp;
    __zn_u32 mxcsr;
    __zn_u32 mxcr_mask;
    struct __fpxreg _st[8];
    struct __xmmreg _xmm[16];
    __zn_u32 __padding[24];
};

typedef struct __mcontext {
    unsigned long gregs[__NGREG];
    struct __fpstate __user* fpregs;
    unsigned long __reserved1[8];
} __mcontext_t;

typedef struct __ucontext {
    unsigned long uc_flags;
    struct __ucontext __user* uc_link;
    struct __stack uc_stack;
    __mcontext_t uc_mcontext;
    __sigset_t uc_sigmask;
    struct __fpstate __fpregs_mem;
    unsigned long __ssp[4];
} __ucontext_t;

#endif
