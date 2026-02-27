#pragma once

#include <stdint.h>

#ifdef __x86_64__
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
#endif

constexpr char ELF_MAG[4] = {0x7F, 'E', 'L', 'F'};

typedef uint64_t elf64_addr_t;
typedef uint64_t elf64_off_t;
typedef uint32_t elf32_addr_t;
typedef uint32_t elf32_off_t;

enum : uint8_t {
    EI_MAG0 = 0,
    EI_MAG1 = 1,
    EI_MAG2 = 2,
    EI_MAG3 = 3,
    EI_CLASS = 4,
    EI_DATA = 5,
    EI_VERSION = 6,
    EI_OSABI = 7,
    EI_ABIVERSION = 8,
    EI_PAD = 9,
    EI_NIDENT = 16,
};

enum : uint16_t {
    EM_X86_64 = 62,
    EM_AARCH64 = 183,
    EM_RISCV = 243,
    EM_LOONGARCH = 258,
};

enum : uint32_t {
    ELFCLASS32 = 1,
    ELFCLASS64 = 2,
};

enum : uint32_t {
    ELFDATA2LSB = 1,
    ELFDATA2MSB = 2,
};

enum : uint32_t {
    EV_NONE = 0,
    EV_CURRENT = 1,
    EV_NUM = 2,
};

enum : uint32_t {
    ELFOSABI_SYSV = 0,
    ELFOSABI_HPUX = 1,
    ELFOSABI_STANDALONE = 255,
};

enum : uint32_t {
    ET_NONE = 0,
    ET_REL = 1,
    ET_EXEC = 2,
    ET_DYN = 3,
    ET_CORE = 4,
};

enum : uint32_t {
    PT_NULL = 0x00000000,
    PT_LOAD = 0x00000001,
    PT_DYNAMIC = 0x00000002,
    PT_INTERP = 0x00000003,
    PT_NOTE = 0x00000004,
    PT_SHLIB = 0x00000005,
    PT_PHDR = 0x00000006,
    PT_TLS = 0x00000007,
};

enum : uint32_t {
    PF_X = 0x00000001,
    PF_W = 0x00000002,
    PF_R = 0x00000004,
};

enum : uint32_t {
    DT_NULL = 0,
    DT_NEEDED = 1,
    DT_PLTRELSZ = 2,
    DT_PLTGOT = 3,
    DT_HASH = 4,
    DT_STRTAB = 5,
    DT_SYMTAB = 6,
    DT_RELA = 7,
    DT_RELASZ = 8,
    DT_RELAENT = 9,
    DT_STRSZ = 10,
    DT_SYMENT = 11,
    DT_INIT = 12,
    DT_FINI = 13,
    DT_SONAME = 14,
    DT_RPATH = 15,
    DT_SYMBOLIC = 16,
    DT_REL = 17,
    DT_RELSZ = 18,
    DT_RELENT = 19,
    DT_PLTREL = 20,
    DT_DEBUG = 21,
    DT_TEXTREL = 22,
    DT_JMPREL = 23,
    DT_BIND_NOW = 24,
    DT_INIT_ARRAY = 25,
    DT_FINI_ARRAY = 26,
    DT_INIT_ARRAYSZ = 27,
    DT_FINI_ARRAYSZ = 28,
    DT_LOOS = 0x60000000,
    DT_HIOS = 0x6FFFFFFF,
    DT_LOPROC = 0x70000000,
    DT_HIPROC = 0x7FFFFFFF,
};

enum : uint32_t {
    AT_NULL = 0,
    AT_IGNORE = 1,
    AT_EXECFD = 2,
    AT_PHDR = 3,
    AT_PHENT = 4,
    AT_PHNUM = 5,
    AT_PAGESZ = 6,
    AT_BASE = 7,
    AT_FLAGS = 8,
    AT_ENTRY = 9,
    AT_NOTELF = 10,
    AT_UID = 11,
    AT_EUID = 12,
    AT_GID = 13,
    AT_EGID = 14,
    AT_HWCAP = 16,
    AT_SECURE = 23,
    AT_RANDOM = 25,
    AT_EXECFN = 31,
    AT_SYSINFO_EHDR = 33,
    AT_INIT_HANDLE = 0x1000,
};

struct elf64_ehdr {
    uint8_t e_ident[EI_NIDENT];
    uint16_t e_type;
    uint16_t e_machine;
    uint32_t e_version;
    elf64_addr_t e_entry;
    elf64_off_t e_phoff;
    elf64_off_t e_shoff;
    uint32_t e_flags;
    uint16_t e_ehsize;
    uint16_t e_phentsize;
    uint16_t e_phnum;
    uint16_t e_shentsize;
    uint16_t e_shnum;
    uint16_t e_shstrndx;
};

struct elf32_ehdr {
    uint8_t e_ident[EI_NIDENT];
    uint16_t e_type;
    uint16_t e_machine;
    uint32_t e_version;
    elf32_addr_t e_entry;
    elf32_off_t e_phoff;
    elf32_off_t e_shoff;
    uint32_t e_flags;
    uint16_t e_ehsize;
    uint16_t e_phentsize;
    uint16_t e_phnum;
    uint16_t e_shentsize;
    uint16_t e_shnum;
    uint16_t e_shstrndx;
};

struct elf64_phdr {
    uint32_t p_type;
    uint32_t p_flags;
    elf64_off_t p_offset;
    elf64_addr_t p_vaddr;
    elf64_addr_t p_paddr;
    uint64_t p_filesz;
    uint64_t p_memsz;
    uint64_t p_align;
};

struct elf32_phdr {
    uint32_t p_type;
    elf32_off_t p_offset;
    elf32_addr_t p_vaddr;
    elf32_addr_t p_paddr;
    uint32_t p_filesz;
    uint32_t p_memsz;
    uint32_t p_flags;
    uint32_t p_align;
};

struct elf64_dyn {
    int64_t d_tag;
    union {
        uint64_t d_val;
        elf64_addr_t d_ptr;
    } d_un;
};

struct elf32_dyn {
    int32_t r_offset;
    union {
        uint32_t d_val;
        elf32_addr_t d_ptr;
    } d_un;
};

struct elf64_nhdr {
    uint64_t n_namesz;
    uint64_t n_descsz;
    uint64_t n_type;
};

struct elf32_nhdr {
    uint32_t n_namesz;
    uint32_t n_descsz;
    uint32_t n_type;
};

struct elf64_auxv {
    uint64_t atype;
    uint64_t avalue;
};

struct elf32_auxv {
    uint32_t atype;
    uint32_t avalue;
};

struct elf_hash {
    uint32_t n_buckets;
    uint32_t n_chain;
};

struct elf32_sym {
    elf32_off_t st_name;
    elf32_addr_t st_value;
    elf32_off_t st_size;
    uint8_t st_info;
    uint8_t st_other;
    uint16_t st_shndx;
};

struct elf64_sym {
    uint32_t st_name;
    uint8_t st_info;
    uint8_t st_other;
    uint16_t st_shndx;
    uint64_t st_value;
    uint64_t st_size;
};

static uint8_t ELF64_ST_BIND(uint8_t info) {
    return info >> 4;
}

static inline uint8_t ELF64_ST_TYPE(uint8_t info) {
    return info & 0x0F;
}

static inline uint8_t ELF64_ST_INFO(uint8_t bind, uint8_t type) {
    return (bind << 4) | type;
}

typedef struct {
    uint16_t si_boundto;
    uint16_t si_flags;
} elf64_syminfo;

static inline uint8_t ELF32_ST_BIND(uint8_t info) {
    return info >> 4;
}

static inline uint8_t ELF32_ST_TYPE(uint8_t info) {
    return info & 0xF;
}

static inline uint8_t ELF32_ST_INFO(uint8_t bind, uint8_t type) {
    return (bind << 4) | (type & 0xF);
}

enum {
    STB_GLOBAL = 1,
    STB_WEAK = 2,
    STB_GNU_UNIQUE = 10,
    STB_LOPROC = 13,
    STB_HIPROC = 15
};

enum {
    STT_NOTYPE = 0,
    STT_OBJECT = 1,
    STT_FUNC = 2,
    STT_TLS = 6,
    STT_GNU_IFUNC = 10,
    STT_LOPROC = 13,
    STT_HIPROC = 15
};

struct elf32_rela {
    elf32_addr_t r_offset;
    uint32_t r_info;
    int32_t r_addend;
};

struct elf64_rela {
    elf64_addr_t r_offset;
    uint64_t r_info;
    int64_t r_addend;
};

struct elf32_rel {
    elf32_addr_t r_offset;
    uint32_t r_info;
};

struct elf64_rel {
    elf64_addr_t r_offset;
    uint64_t r_info;
};

static inline uint32_t ELF32_R_SYM(uint32_t info) {
    return info >> 8;
}

static inline uint32_t ELF32_R_TYPE(uint32_t info) {
    return info & 0xF;
}

static inline uint32_t ELF64_R_SYM(uint64_t info) {
    return info >> 32;
}

static inline uint32_t ELF64_R_TYPE(uint64_t info) {
    return info & 0xFFFFFFFF;
}
