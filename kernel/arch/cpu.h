// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>

#if defined(__x86_64__)
#include <arch/x86_64/bits/cpu.h>
#elif defined(__aarch64__)
#include <arch/aarch64/bits/cpu.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <arch/riscv64/bits/cpu.h>
#elif defined(__loongarch__) && __loongarch_grlen == 64
#include <arch/loongarch64/bits/cpu.h>
#else
#error "Unsupported architecture"
#endif

constexpr size_t ARCH_CACHE_LINE_SIZE = __ARCH_CACHE_LINE_SIZE;
