// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/elf.h>

#define __ARCH_ELF_CLASS   ELFCLASS64
#define __ARCH_ELF_DATA    ELFDATA2LSB
#define __ARCH_ELF_MACHINE EM_X86_64

#define __ARCH_ELF_R_SYM   ELF64_R_SYM
#define __ARCH_ELF_R_TYPE  ELF64_R_TYPE
#define __ARCH_ELF_ST_BIND ELF64_ST_BIND
#define __ARCH_ELF_ST_TYPE ELF64_ST_TYPE

typedef elf64_ehdr_t __arch_elf_ehdr_t;
typedef elf64_phdr_t __arch_elf_phdr_t;
typedef elf64_dyn_t __arch_elf_dyn_t;
typedef elf64_addr_t __arch_elf_addr_t;
typedef elf64_off_t __arch_elf_off_t;
typedef elf64_nhdr_t __arch_elf_nhdr_t;
typedef elf64_auxv_t __arch_elf_auxv_t;
typedef elf64_sym_t __arch_elf_sym_t;
typedef elf64_rela_t __arch_elf_rela_t;
typedef elf64_rel_t __arch_elf_rel_t;
typedef elf64_relr_t __arch_elf_relr_t;

enum {
    __ARCH_R_NONE = 0,
    __ARCH_R_64 = 1,
    __ARCH_R_COPY = 5,
    __ARCH_R_GLOB_DAT = 6,
    __ARCH_R_JUMP_SLOT = 7,
    __ARCH_R_RELATIVE = 8,
    __ARCH_R_IRELATIVE = 37,
};
