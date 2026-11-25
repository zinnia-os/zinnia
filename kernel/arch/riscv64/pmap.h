// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

constexpr uint64_t ARCH_FLAG_VALID = 1 << 0;
constexpr uint64_t ARCH_FLAG_READ = 1 << 1;
constexpr uint64_t ARCH_FLAG_WRITE = 1 << 2;
constexpr uint64_t ARCH_FLAG_EXECUTE = 1 << 3;
constexpr uint64_t ARCH_FLAG_USER = 1 << 4;
constexpr uint64_t ARCH_FLAG_GLOBAL = 1 << 5;
constexpr uint64_t ARCH_FLAG_ACCESSED = 1 << 6;
constexpr uint64_t ARCH_FLAG_DIRTY = 1 << 7;
constexpr uint64_t ARCH_PTE_PPN_SHIFT = 10;
constexpr uint64_t ARCH_PTE_PPN_MASK = 0x003F'FFFF'FFFF'FC00;
constexpr uint64_t ARCH_SATP_MODE_SHIFT = 60;
constexpr uint64_t ARCH_SATP_MODE_SV39 = 8;
constexpr uint64_t ARCH_SATP_MODE_SV48 = 9;

constexpr uintptr_t ARCH_SV39_HHDM = 0xFFFF'FFC0'0000'0000;
constexpr uintptr_t ARCH_SV39_PFNDB = 0xFFFF'FFE0'0000'0000;
constexpr uintptr_t ARCH_SV39_MAPPING = 0xFFFF'FFE4'0000'0000;
constexpr uintptr_t ARCH_SV39_USER_END = 0x0000'003F'FFFF'F000;

constexpr uintptr_t ARCH_SV48_HHDM = 0xFFFF'8000'0000'0000;
constexpr uintptr_t ARCH_SV48_PFNDB = 0xFFFF'A000'0000'0000;
constexpr uintptr_t ARCH_SV48_MAPPING = 0xFFFF'C000'0000'0000;
constexpr uintptr_t ARCH_SV48_USER_END = 0x0000'7FFF'FFFF'F000;

static inline uint64_t arch_satp_build(uint8_t levels, uint64_t root) {
    return (ARCH_SATP_MODE_SV39 + levels - 3) << ARCH_SATP_MODE_SHIFT | root >> 12;
}

static inline uint8_t arch_satp_levels(uint64_t satp) {
    return (satp >> ARCH_SATP_MODE_SHIFT) - ARCH_SATP_MODE_SV39 + 3;
}
