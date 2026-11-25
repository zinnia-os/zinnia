// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

enum sstatus_bits : uint64_t {
    SSTATUS_SIE = 1 << 1,
    SSTATUS_SPIE = 1 << 5,
    SSTATUS_UBE = 1 << 6,
    SSTATUS_SPP = 1 << 8,
    SSTATUS_VS_MASK = 3 << 9,
    SSTATUS_VS_OFF = 0 << 9,
    SSTATUS_VS_INITIAL = 1 << 9,
    SSTATUS_VS_CLEAN = 2 << 9,
    SSTATUS_VS_DIRTY = 3 << 9,
    SSTATUS_FS_MASK = 3 << 13,
    SSTATUS_FS_OFF = 0 << 13,
    SSTATUS_FS_INITIAL = 1 << 13,
    SSTATUS_FS_CLEAN = 2 << 13,
    SSTATUS_FS_DIRTY = 3 << 13,
    SSTATUS_XS_MASK = 3 << 15,
    SSTATUS_SUM = 1 << 18,
    SSTATUS_MXR = 1 << 19,
    SSTATUS_SPELP = 1 << 23,
    SSTATUS_SDT = 1 << 24,
    SSTATUS_UXL_MASK = 3ull << 32,
    SSTATUS_SD = 1ull << 63,
};

enum sie_bits : uint64_t {
    SIE_SSIE = 1 << 1,
    SIE_STIE = 1 << 5,
    SIE_SEIE = 1 << 9,
    SIE_LCOFIE = 1 << 13,
};

enum scause_bits : uint64_t {
    SCAUSE_INTERRUPT = 1ull << 63,
};

enum riscv_irq : uint64_t {
    IRQ_S_SOFT = 1,
    IRQ_S_TIMER = 5,
    IRQ_S_EXT = 9,
    IRQ_COUNTER_OVERFLOW = 13,
};

enum riscv_exception : uint64_t {
    EXC_FETCH_MISALIGNED = 0,
    EXC_FETCH_ACCESS = 1,
    EXC_ILLEGAL_INSTRUCTION = 2,
    EXC_BREAKPOINT = 3,
    EXC_LOAD_MISALIGNED = 4,
    EXC_LOAD_ACCESS = 5,
    EXC_STORE_MISALIGNED = 6,
    EXC_STORE_ACCESS = 7,
    EXC_ECALL_U = 8,
    EXC_ECALL_S = 9,
    EXC_FETCH_PAGE_FAULT = 12,
    EXC_LOAD_PAGE_FAULT = 13,
    EXC_STORE_PAGE_FAULT = 15,
    EXC_DOUBLE_TRAP = 16,
    EXC_SOFTWARE_CHECK = 18,
    EXC_HARDWARE_ERROR = 19,
};
