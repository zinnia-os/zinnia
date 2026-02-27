#include <zinnia/mem.h>
#include <zinnia/system.h>
#include <zinnia/thread.h>
#include <common/compiler.h>
#include <common/elf.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

void* __zinnia_vdsosym(uintptr_t vdso_base, const char* string, const char* version) {
    if (!string)
        return nullptr;
    if (!vdso_base)
        return nullptr;

    struct elf_ehdr* ehdr = (struct elf_ehdr*)(vdso_base);
    struct elf_dyn* dynamic = nullptr;
    uintptr_t actual_base = 0;

    for (size_t i = 0; i < ehdr->e_phnum; i++) {
        struct elf_phdr* phdr = (struct elf_phdr*)(vdso_base + ehdr->e_phoff + i * ehdr->e_phentsize);
        if (phdr->p_type == PT_LOAD)
            actual_base = vdso_base - phdr->p_vaddr + phdr->p_offset;
        if (phdr->p_type == PT_DYNAMIC)
            dynamic = (struct elf_dyn*)(vdso_base + phdr->p_offset);
    }

    size_t symtab_size = 0;
    struct elf_sym* symtab = nullptr;
    const char* strtab = nullptr;

    while (dynamic->d_tag != DT_NULL) {
        switch (dynamic->d_tag) {
        case DT_SYMTAB:
            symtab = (struct elf_sym*)(actual_base + dynamic->d_un.d_ptr);
            break;
        case DT_STRTAB:
            strtab = (const char*)(actual_base + dynamic->d_un.d_ptr);
            break;
        case DT_HASH:
            symtab_size = ((struct elf_hash*)(actual_base + dynamic->d_un.d_ptr))->n_chain;
            break;
        }
        dynamic++;
    }

    if (!symtab_size || !symtab || !strtab)
        return nullptr;

    for (size_t i = 0; i < symtab_size; i++) {
        // Check if this symbol has one of the accepted types and binds.
        if (!(1 << ELF_ST_TYPE(symtab[i].st_info) &
              (1 << STT_NOTYPE | 1 << STT_OBJECT | 1 << STT_FUNC | 1 << STT_GNU_IFUNC)))
            continue;
        if (!(1 << ELF_ST_BIND(symtab[i].st_info) & (1 << STB_GLOBAL | 1 << STB_WEAK | 1 << STB_GNU_UNIQUE)))
            continue;
        if (strcmp(string, &strtab[symtab[i].st_name]) != 0)
            continue;

        // TODO: Symbol versioning

        uintptr_t addr = actual_base + symtab[i].st_value;
        if (ELF_ST_TYPE(symtab[i].st_info) == STT_GNU_IFUNC)
            addr = ((uintptr_t (*)())addr)();
        return (void*)addr;
    }

    return nullptr;
}

#define VDSO_DEF(name)          static typeof(&name) __vdso_##name = nullptr;
#define VDSO_SYM(name, version) __vdso_##name = __zinnia_vdsosym(base, "__vdso_" #name, "ZINNIA_" #version)

VDSO_DEF(zn_log)
VDSO_DEF(zn_vmo_map)
VDSO_DEF(zn_thread_exit)

void __zinnia_vdso_load(uintptr_t base) {
    VDSO_SYM(zn_log, 1);
    VDSO_SYM(zn_vmo_map, 1);
    VDSO_SYM(zn_thread_exit, 1);
}

void zn_log(const char* message, size_t len) {
    __vdso_zn_log(message, len);
}

zn_status_t zn_vmo_map(
    zn_handle_t vmo,
    zn_handle_t vas,
    uintptr_t vmo_offset,
    uintptr_t* addr,
    size_t bytes,
    enum zn_vm_flags flags
) {
    return __vdso_zn_vmo_map(vmo, vas, vmo_offset, addr, bytes, flags);
}

void zn_thread_exit() {
    __vdso_zn_thread_exit();
}
