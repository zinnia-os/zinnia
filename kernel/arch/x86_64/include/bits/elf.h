// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/elf.h>

#define ARCH_ELF_BITS    64
#define ARCH_ELF_DATA    ELFDATA2LSB
#define ARCH_ELF_MACHINE EM_X86_64

enum {
    ARCH_R_NONE = 0,
    ARCH_R_64 = 1,
    ARCH_R_COPY = 5,
    ARCH_R_GLOB_DAT = 6,
    ARCH_R_JUMP_SLOT = 7,
    ARCH_R_RELATIVE = 8,
    ARCH_R_IRELATIVE = 37,
};
