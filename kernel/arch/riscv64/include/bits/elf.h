// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/elf.h>

#define ARCH_ELF_BITS    64
#define ARCH_ELF_DATA    ELFDATA2LSB
#define ARCH_ELF_MACHINE EM_RISCV

enum {
    ARCH_R_NONE = 0,
    ARCH_R_64 = 2,
    ARCH_R_COPY = 4,
    ARCH_R_GLOB_DAT = 2,
    ARCH_R_JUMP_SLOT = 5,
    ARCH_R_RELATIVE = 3,
    ARCH_R_IRELATIVE = 58,
};
