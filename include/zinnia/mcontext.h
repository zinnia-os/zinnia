/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_MCONTEXT_H
#define __ZINNIA_MCONTEXT_H

#include <zinnia/types.h>

#define __MC_TAG_END 0

struct __mcontext_tag {
    __zn_u32 magic;
    __zn_u32 size;
    __zn_u64 offset;
};

#if defined(__x86_64__)
#include <zinnia/x86_64/mcontext.h>
#elif defined(__aarch64__)
#include <zinnia/aarch64/mcontext.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <zinnia/riscv64/mcontext.h>
#else
#error "Unsupported architecture"
#endif

#endif
