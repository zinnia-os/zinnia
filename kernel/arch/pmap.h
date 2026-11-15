// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#if defined(__x86_64__)
#include <arch/x86_64/bits/pmap.h>
#elif defined(__aarch64__)
#include <arch/aarch64/bits/pmap.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <arch/riscv64/bits/pmap.h>
#elif defined(__loongarch__) && __loongarch_grlen == 64
#include <arch/loongarch64/bits/pmap.h>
#else
#error "Unsupported architecture"
#endif

typedef __arch_pte_t pte_t;
