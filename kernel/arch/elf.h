// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#if defined(__x86_64__)
#include <arch/x86_64/bits/elf.h>
#elif defined(__aarch64__)
#include <arch/aarch64/bits/elf.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <arch/riscv64/bits/elf.h>
#elif defined(__loongarch__) && __loongarch_grlen == 64
#include <arch/loongarch64/bits/elf.h>
#else
#error "Unsupported architecture"
#endif

#define ARCH_ELF_R_SYM   __ARCH_ELF_R_SYM
#define ARCH_ELF_R_TYPE  __ARCH_ELF_R_TYPE
#define ARCH_ELF_ST_BIND __ARCH_ELF_ST_BIND
#define ARCH_ELF_ST_TYPE __ARCH_ELF_ST_TYPE

typedef __arch_elf_ehdr_t elf_ehdr_t;
typedef __arch_elf_phdr_t elf_phdr_t;
typedef __arch_elf_dyn_t elf_dyn_t;
typedef __arch_elf_addr_t elf_addr_t;
typedef __arch_elf_off_t elf_off_t;
typedef __arch_elf_nhdr_t elf_nhdr_t;
typedef __arch_elf_auxv_t elf_auxv_t;
typedef __arch_elf_sym_t elf_sym_t;
typedef __arch_elf_rela_t elf_rela_t;
typedef __arch_elf_rel_t elf_rel_t;
typedef __arch_elf_relr_t elf_relr_t;

enum {
    ARCH_ELF_CLASS = __ARCH_ELF_CLASS,
    ARCH_ELF_DATA = __ARCH_ELF_DATA,
    ARCH_ELF_MACHINE = __ARCH_ELF_MACHINE,
};

enum {
    ARCH_R_NONE = __ARCH_R_NONE,
    ARCH_R_64 = __ARCH_R_64,
    ARCH_R_COPY = __ARCH_R_COPY,
    ARCH_R_GLOB_DAT = __ARCH_R_GLOB_DAT,
    ARCH_R_JUMP_SLOT = __ARCH_R_JUMP_SLOT,
    ARCH_R_RELATIVE = __ARCH_R_RELATIVE,
    ARCH_R_IRELATIVE = __ARCH_R_IRELATIVE,
};
