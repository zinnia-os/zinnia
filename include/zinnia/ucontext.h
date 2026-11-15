/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_UCONTEXT_H
#define __ZINNIA_UCONTEXT_H

#if defined(__x86_64__)
#include <zinnia/x86_64/ucontext.h>
#elif defined(__aarch64__)
#include <zinnia/aarch64/ucontext.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <zinnia/riscv64/ucontext.h>
#else
#error "Unsupported architecture"
#endif

#endif
