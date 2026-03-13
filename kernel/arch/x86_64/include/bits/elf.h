#pragma once

#define ELF_ARCH_CLASS   ELFCLASS64
#define ELF_ARCH_DATA    ELFDATA2LSB
#define ELF_ARCH_MACHINE EM_X86_64

#define ELF_R_SYM   ELF64_R_SYM
#define ELF_R_TYPE  ELF64_R_TYPE
#define ELF_ST_BIND ELF64_ST_BIND
#define ELF_ST_TYPE ELF64_ST_TYPE

#define elf_ehdr elf64_ehdr
#define elf_phdr elf64_phdr
#define elf_dyn  elf64_dyn
#define elf_addr elf64_addr
#define elf_off  elf64_off
#define elf_nhdr elf64_nhdr
#define elf_auxv elf64_auxv
#define elf_sym  elf64_sym
#define elf_rela elf64_rela
#define elf_rel  elf64_rel

enum {
    R_NONE = 0,
    R_COPY = 5,
    R_GLOB_DAT = 6,
    R_JUMP_SLOT = 7,
    R_RELATIVE = 8,
    R_IRELATIVE = 37,
};
