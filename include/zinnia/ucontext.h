/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_UCONTEXT_H
#define __ZINNIA_UCONTEXT_H

#include <zinnia/compiler.h>
#include <zinnia/mcontext.h>
#include <zinnia/signal.h>
#include <zinnia/types.h>

#define __UC_SIGMASK (1 << 0)
#define __UC_STACK   (1 << 1)
#define __UC_CPU     (1 << 2)
#define __UC_TLSBASE (1 << 3)
#define __UC_TAGS    (1 << 4)

typedef struct __ucontext {
    __zn_u32 uc_flags;
    __zn_u32 __pad;
    struct __ucontext __user* uc_link;
    __sigset_t uc_sigmask;
    struct __stack uc_stack;
    __mcontext_t uc_mcontext;
} __ucontext_t;

#endif
