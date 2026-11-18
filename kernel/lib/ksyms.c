// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/compiler.h>
#include <lib/elf.h>
#include <lib/hashmap.h>
#include <lib/init.h>
#include <lib/ksyms.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <string.h>

[[__visibility("hidden")]]
extern const elf_sym_t __ld_dynsym_start[];
[[__visibility("hidden")]]
extern const elf_sym_t __ld_dynsym_end[];
[[__visibility("hidden")]]
extern const char __ld_dynstr_start[];
[[__visibility("hidden")]]
extern const char __ld_dynstr_end[];
[[__visibility("hidden")]]
extern const elf_dyn_t _DYNAMIC[];

static uintptr_t slide;
static bool slide_known;
static const char* kernel_name;
static hashmap(const char*, uintptr_t) symbols;

static uint64_t sym_hash(const void* key, size_t) {
    const char* name = *(const char* const*)key;
    return hashmap_hash_default(name, strlen(name));
}

static bool sym_eq(const void* a, const void* b, size_t) {
    return !strcmp(*(const char* const*)a, *(const char* const*)b);
}

static size_t num_syms() {
    return __ld_dynsym_end - __ld_dynsym_start;
}

static const char* dynstr(uint64_t offset) {
    if (offset >= (size_t)(__ld_dynstr_end - __ld_dynstr_start))
        return nullptr;
    return &__ld_dynstr_start[offset];
}

static uintptr_t kernel_slide() {
    if (slide_known)
        return slide;

    for (size_t i = 0; i < num_syms(); i++) {
        const char* name = dynstr(__ld_dynsym_start[i].st_name);
        if (name != nullptr && !strcmp(name, "ksym_lookup")) {
            slide = (uintptr_t)ksym_lookup - __ld_dynsym_start[i].st_value;
            break;
        }
    }
    slide_known = true;
    return slide;
}

void ksym_init() {
    const uintptr_t base = kernel_slide();

    for (const elf_dyn_t* dyn = _DYNAMIC; dyn->d_tag != DT_NULL; dyn++) {
        if (dyn->d_tag == DT_SONAME)
            kernel_name = dynstr(dyn->d_un.d_val);
    }

    hashmap_init(&symbols);

    size_t count = 0;
    for (size_t i = 0; i < num_syms(); i++) {
        const elf_sym_t* sym = &__ld_dynsym_start[i];
        if (sym->st_shndx == SHN_UNDEF)
            continue;

        const char* name = dynstr(sym->st_name);
        if (name == nullptr || *name == 0)
            continue;

        const errno_t status = hashmap_insert(&symbols, name, sym->st_value + base, sym_hash, sym_eq);
        assert(status == 0, "Failed to register kernel symbol \"%s\" (%i)", name, status);
        count++;
    }

    log("ksyms: Registered %zu kernel symbols\n", count);
}

bool ksym_find(const char* name, uintptr_t* addr) {
    uintptr_t* value = hashmap_get(&symbols, name, sym_hash, sym_eq);
    if (value == nullptr)
        return false;
    *addr = *value;
    return true;
}

const char* ksym_kernel_name() {
    return kernel_name;
}

bool ksym_lookup(uintptr_t addr, const char** name, size_t* offset) {
    addr -= kernel_slide();
    for (size_t i = 0; i < num_syms(); i++) {
        const elf_sym_t* sym = &__ld_dynsym_start[i];
        if (ELF_ST_TYPE(sym->st_info) != STT_FUNC || sym->st_shndx == SHN_UNDEF)
            continue;
        if (addr >= sym->st_value && addr - sym->st_value < sym->st_size) {
            const char* sym_name = dynstr(sym->st_name);
            if (sym_name == nullptr)
                return false;
            *name = sym_name;
            *offset = addr - sym->st_value;
            return true;
        }
    }
    return false;
}

void ksym_format(uintptr_t addr, char* buf, size_t size) {
    const char* name;
    size_t offset;
    if (ksym_lookup(addr, &name, &offset))
        ksnprintf(buf, size, "%s+0x%zx", name, offset);
    else
        ksnprintf(buf, size, "<\?\?\?>");
}

[[__init]]
static void ksyms_stage_init(struct boot_info*) {
    ksym_init();
}
INIT_TASK(ksyms_stage, "lib.ksyms", ksyms_stage_init);
INIT_DEPENDS(ksyms_stage, mem_stage);
