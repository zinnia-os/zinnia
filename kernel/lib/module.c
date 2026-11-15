// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "module: " fmt

#include <arch/elf.h>
#include <lib/assert.h>
#include <lib/cmdline.h>
#include <lib/elf.h>
#include <lib/ksyms.h>
#include <lib/module.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/spin.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stdckdint.h>
#include <string.h>

extern errno_t (*__ld_mod_entry_start[])(struct module*);
extern errno_t (*__ld_mod_entry_end[])(struct module*);

static struct spinlock module_lock;
static tailq_head(struct module) module_list;

static bool in_range(size_t total, uint64_t offset, uint64_t size) {
    return offset <= total && size <= total - offset;
}

static void* image_ptr(struct module* mod, uint64_t vaddr, uint64_t size) {
    uintptr_t addr, end;
    if (ckd_add(&addr, mod->bias, vaddr) || ckd_add(&end, addr, size))
        return nullptr;

    for (size_t i = 0; i < mod->num_segments; i++) {
        const struct module_segment* seg = &mod->segments[i];
        if (addr >= seg->virt && end <= seg->virt + seg->size)
            return (void*)addr;
    }
    return nullptr;
}

static const char* image_cstr(struct module* mod, uint64_t vaddr, uint64_t size) {
    const char* str = image_ptr(mod, vaddr, size);
    if (str == nullptr || size == 0 || strnlen(str, size) >= size)
        return nullptr;
    return str;
}

static const char* table_str(const char* table, size_t size, uint64_t offset) {
    if (offset >= size)
        return nullptr;
    const char* str = table + offset;
    if (strnlen(str, size - offset) >= size - offset)
        return nullptr;
    return str;
}

static const char* sym_name(struct module* mod, uint64_t offset) {
    return table_str(mod->strtab, mod->strtab_size, offset);
}

static struct module* find_locked(const char* name) {
    for (struct module* m = tailq_first(&module_list); m != nullptr; m = tailq_next(m, link)) {
        if (!strcmp(m->name, name))
            return m;
    }
    return nullptr;
}

void module_init() {
    spin_init(&module_lock);
    tailq_init(&module_list);

    for (auto start = __ld_mod_entry_start; start < __ld_mod_entry_end; start += 1) {
        errno_t mod_status = (*start)(nullptr);
        assert(mod_status == 0, "Built-in entry point returned %i", mod_status);
    }
}

static errno_t resolve_symbol(struct module* mod, uint32_t index, uintptr_t* out) {
    if (index >= mod->num_syms)
        return ENOEXEC;

    const elf_sym_t* sym = &((const elf_sym_t*)mod->symtab)[index];
    if (sym->st_shndx != SHN_UNDEF) {
        *out = mod->bias + sym->st_value;
        return 0;
    }

    const char* name = sym_name(mod, sym->st_name);
    if (name == nullptr)
        return ENOEXEC;

    if (ksym_find(name, out))
        return 0;

    if (ARCH_ELF_ST_BIND(sym->st_info) == STB_WEAK) {
        *out = 0;
        return 0;
    }

    log_err("%s: Unresolved symbol \"%s\"\n", mod->name, name);
    return ENOENT;
}

static errno_t store_word(struct module* mod, uint64_t vaddr, uintptr_t value) {
    uintptr_t* where = image_ptr(mod, vaddr, sizeof(uintptr_t));
    if (where == nullptr)
        return ENOEXEC;
    *where = value;
    return 0;
}

static errno_t relocate_relative(struct module* mod, uint64_t vaddr) {
    uintptr_t* where = image_ptr(mod, vaddr, sizeof(uintptr_t));
    if (where == nullptr)
        return ENOEXEC;
    *where += mod->bias;
    return 0;
}

static errno_t apply_rela(struct module* mod, uint64_t addr, uint64_t size, uint64_t entsize) {
    if (entsize == 0)
        entsize = sizeof(elf_rela_t);
    if (entsize < sizeof(elf_rela_t))
        return ENOEXEC;

    const uint8_t* table = image_ptr(mod, addr, size);
    if (table == nullptr)
        return ENOEXEC;

    for (uint64_t off = 0; off + sizeof(elf_rela_t) <= size; off += entsize) {
        elf_rela_t rela;
        memcpy(&rela, table + off, sizeof(rela));

        errno_t status = 0;
        uintptr_t value;
        switch (ARCH_ELF_R_TYPE(rela.r_info)) {
        case ARCH_R_NONE:
            break;
        case ARCH_R_64:
        case ARCH_R_GLOB_DAT:
        case ARCH_R_JUMP_SLOT:
            status = resolve_symbol(mod, ARCH_ELF_R_SYM(rela.r_info), &value);
            if (status == 0)
                status = store_word(mod, rela.r_offset, value + rela.r_addend);
            break;
        case ARCH_R_RELATIVE:
            status = store_word(mod, rela.r_offset, mod->bias + rela.r_addend);
            break;
        default:
            log_err("%s: Unsupported relocation type %u\n", mod->name, ARCH_ELF_R_TYPE(rela.r_info));
            status = ENOEXEC;
            break;
        }

        if (status)
            return status;
    }
    return 0;
}

static errno_t apply_relr(struct module* mod, uint64_t addr, uint64_t size, uint64_t entsize) {
    if (entsize != 0 && entsize != sizeof(elf_relr_t))
        return ENOEXEC;

    const elf_relr_t* table = image_ptr(mod, addr, size);
    if (table == nullptr)
        return ENOEXEC;

    uint64_t where = 0;
    for (size_t i = 0; i < size / sizeof(elf_relr_t); i++) {
        elf_relr_t entry = table[i];
        errno_t status;

        if ((entry & 1) == 0) {
            status = relocate_relative(mod, entry);
            if (status)
                return status;
            where = entry + sizeof(elf_relr_t);
        } else {
            for (size_t bit = 0; (entry >>= 1) != 0; bit++) {
                if ((entry & 1) == 0)
                    continue;
                status = relocate_relative(mod, where + bit * sizeof(elf_relr_t));
                if (status)
                    return status;
            }
            where += (sizeof(elf_relr_t) * 8 - 1) * sizeof(elf_relr_t);
        }
    }
    return 0;
}

static void module_free(struct module* mod) {
    const size_t page_size = arch_mem_page_size();

    mem_free(mod->cmdline, mod->cmdline_size);

    for (size_t i = 0; i < mod->num_segments; i++) {
        const struct module_segment* seg = &mod->segments[i];
        for (size_t off = 0; off < seg->size; off += page_size)
            pmap_unmap(&kernel_vma.pmap, seg->virt + off);
    }
    pmap_shootdown(&kernel_vma.pmap);
    for (size_t i = 0; i < mod->num_segments; i++) {
        const struct module_segment* seg = &mod->segments[i];
        mem_phys_free(seg->phys, seg->size / page_size);
    }
    mem_free(mod->segments, mod->max_segments * sizeof(struct module_segment));

    if (mod->base != 0)
        vm_free(mod->base, mod->size);

    mem_free(mod, sizeof(*mod));
}

static enum pte_flags prot_from_phdr(const elf_phdr_t* phdr) {
    enum pte_flags prot = 0;
    if (phdr->p_flags & PF_R)
        prot |= PTE_READ;
    if (phdr->p_flags & PF_W)
        prot |= PTE_WRITE;
    if (phdr->p_flags & PF_X)
        prot |= PTE_EXEC;
    return prot;
}

static errno_t read_headers(const void* data, size_t length, elf_ehdr_t* ehdr, elf_phdr_t** out) {
    const uint8_t* file = data;

    if (data == nullptr || length < sizeof(elf_ehdr_t))
        return ENOEXEC;

    memcpy(ehdr, file, sizeof(*ehdr));

    if (memcmp(ehdr->e_ident, ELF_MAG, sizeof(ELF_MAG)) != 0 || ehdr->e_ident[EI_VERSION] != EV_CURRENT ||
        ehdr->e_ident[EI_CLASS] != ARCH_ELF_CLASS || ehdr->e_ident[EI_DATA] != ARCH_ELF_DATA ||
        ehdr->e_machine != ARCH_ELF_MACHINE || ehdr->e_type != ET_DYN)
        return ENOEXEC;

    if (ehdr->e_phentsize < sizeof(elf_phdr_t) || ehdr->e_phnum == 0 ||
        !in_range(length, ehdr->e_phoff, (uint64_t)ehdr->e_phnum * ehdr->e_phentsize))
        return ENOEXEC;

    elf_phdr_t* phdrs = mem_alloc(ehdr->e_phnum * sizeof(elf_phdr_t), 0);
    if (phdrs == nullptr)
        return ENOMEM;
    for (size_t i = 0; i < ehdr->e_phnum; i++)
        memcpy(&phdrs[i], file + ehdr->e_phoff + i * ehdr->e_phentsize, sizeof(elf_phdr_t));

    *out = phdrs;
    return 0;
}

errno_t module_load(const void* data, size_t length, const char* cmdline, struct module** out) {
    const uint8_t* file = data;
    const size_t page_size = arch_mem_page_size();

    elf_ehdr_t ehdr;
    elf_phdr_t* phdrs;
    errno_t header_status = read_headers(data, length, &ehdr, &phdrs);
    if (header_status)
        return header_status;

    struct module* mod = mem_alloc(sizeof(struct module), 0);
    if (mod == nullptr) {
        mem_free(phdrs, ehdr.e_phnum * sizeof(elf_phdr_t));
        return ENOMEM;
    }
    mod->version = "";
    mod->author = "";
    mod->desc = "";
    mod->license = "";

    errno_t status = ENOEXEC;
    bool listed = false;

    uintptr_t load_min = UINTPTR_MAX;
    uintptr_t load_end = 0;
    size_t num_loads = 0;
    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        const elf_phdr_t* phdr = &phdrs[i];
        if (phdr->p_type != PT_LOAD || phdr->p_memsz == 0)
            continue;
        if (phdr->p_filesz > phdr->p_memsz || !in_range(length, phdr->p_offset, phdr->p_filesz))
            goto fail;

        uintptr_t end;
        if (ckd_add(&end, phdr->p_vaddr, phdr->p_memsz) || end > UINTPTR_MAX - page_size)
            goto fail;

        const uintptr_t start = ROUND_DOWN(phdr->p_vaddr, page_size);
        if (start < load_end)
            goto fail;
        if (load_min == UINTPTR_MAX)
            load_min = start;
        load_end = ROUND_UP(end, page_size);
        num_loads++;
    }

    if (num_loads == 0)
        goto fail;

    status = ENOMEM;
    mod->segments = mem_alloc(num_loads * sizeof(struct module_segment), 0);
    if (mod->segments == nullptr)
        goto fail;
    mod->max_segments = num_loads;

    mod->base = vm_alloc(load_end - load_min);
    if (mod->base == 0)
        goto fail;
    mod->size = load_end - load_min;
    mod->bias = mod->base - load_min;

    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        const elf_phdr_t* phdr = &phdrs[i];
        if (phdr->p_type != PT_LOAD || phdr->p_memsz == 0)
            continue;

        const uintptr_t start = ROUND_DOWN(phdr->p_vaddr, page_size);
        const size_t size = ROUND_UP(phdr->p_vaddr + phdr->p_memsz, page_size) - start;

        phys_t phys;
        status = mem_phys_alloc(size / page_size, 0, &phys);
        if (status)
            goto fail;

        struct module_segment* seg = &mod->segments[mod->num_segments++];
        *seg = (struct module_segment){.phys = phys, .virt = mod->bias + start, .size = size};

        for (size_t off = 0; off < size; off += page_size) {
            status = pmap_map(&kernel_vma.pmap, seg->virt + off, phys + off, PTE_READ | PTE_WRITE, CACHE_WRITE_BACK);
            if (status)
                goto fail;
        }

        memcpy((void*)(mod->bias + phdr->p_vaddr), file + phdr->p_offset, phdr->p_filesz);
    }

    status = ENOEXEC;

    const elf_dyn_t* dyn = nullptr;
    size_t num_dyn = 0;
    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        if (phdrs[i].p_type != PT_DYNAMIC)
            continue;
        dyn = image_ptr(mod, phdrs[i].p_vaddr, phdrs[i].p_memsz);
        num_dyn = phdrs[i].p_memsz / sizeof(elf_dyn_t);
        break;
    }
    if (dyn == nullptr)
        goto fail;

    uint64_t dt_strtab = 0, dt_strsz = 0, dt_symtab = 0, dt_hash = 0, dt_soname = 0;
    uint64_t dt_rela = 0, dt_relasz = 0, dt_relaent = 0;
    uint64_t dt_jmprel = 0, dt_pltrelsz = 0, dt_pltrel = DT_RELA;
    uint64_t dt_relr = 0, dt_relrsz = 0, dt_relrent = 0;
    bool has_strtab = false, has_symtab = false, has_hash = false, has_soname = false;
    bool has_rela = false, has_jmprel = false, has_relr = false, has_needed = false;

    for (size_t i = 0; i < num_dyn && dyn[i].d_tag != DT_NULL; i++) {
        const uint64_t val = dyn[i].d_un.d_val;
        switch (dyn[i].d_tag) {
        case DT_STRTAB:
            dt_strtab = val;
            has_strtab = true;
            break;
        case DT_STRSZ:
            dt_strsz = val;
            break;
        case DT_SYMTAB:
            dt_symtab = val;
            has_symtab = true;
            break;
        case DT_HASH:
            dt_hash = val;
            has_hash = true;
            break;
        case DT_SONAME:
            dt_soname = val;
            has_soname = true;
            break;
        case DT_NEEDED:
            has_needed = true;
            break;
        case DT_RELA:
            dt_rela = val;
            has_rela = true;
            break;
        case DT_RELASZ:
            dt_relasz = val;
            break;
        case DT_RELAENT:
            dt_relaent = val;
            break;
        case DT_JMPREL:
            dt_jmprel = val;
            has_jmprel = true;
            break;
        case DT_PLTRELSZ:
            dt_pltrelsz = val;
            break;
        case DT_PLTREL:
            dt_pltrel = val;
            break;
        case DT_RELR:
            dt_relr = val;
            has_relr = true;
            break;
        case DT_RELRSZ:
            dt_relrsz = val;
            break;
        case DT_RELRENT:
            dt_relrent = val;
            break;
        default:
            break;
        }
    }

    if (!has_strtab || !has_symtab || !has_hash || !has_soname || dt_strsz == 0 || dt_pltrel != DT_RELA)
        goto fail;

    const elf_hash_t* hash = image_ptr(mod, dt_hash, sizeof(elf_hash_t));
    if (hash == nullptr)
        goto fail;

    mod->num_syms = hash->n_chain;
    mod->symtab = image_ptr(mod, dt_symtab, (uint64_t)mod->num_syms * sizeof(elf_sym_t));
    mod->strtab = image_ptr(mod, dt_strtab, dt_strsz);
    mod->strtab_size = dt_strsz;
    if (mod->symtab == nullptr || mod->strtab == nullptr) {
        mod->num_syms = 0;
        goto fail;
    }

    mod->name = sym_name(mod, dt_soname);
    if (mod->name == nullptr || *mod->name == 0)
        goto fail;

    for (size_t i = 0; has_needed && i < num_dyn && dyn[i].d_tag != DT_NULL; i++) {
        if (dyn[i].d_tag != DT_NEEDED)
            continue;

        // TODO: Modules can technically claim they have dependencies on other modules, but we don't support that yet.
        const char* needed = sym_name(mod, dyn[i].d_un.d_val);
        const char* kernel_name = ksym_kernel_name();
        if (needed == nullptr || kernel_name == nullptr || strcmp(needed, kernel_name) != 0) {
            log_err("%s: Modules can only depend on the kernel, not \"%s\"\n", mod->name, needed ? needed : "?");
            goto fail;
        }
    }

    if (has_relr) {
        status = apply_relr(mod, dt_relr, dt_relrsz, dt_relrent);
        if (status)
            goto fail;
    }
    if (has_rela) {
        status = apply_rela(mod, dt_rela, dt_relasz, dt_relaent);
        if (status)
            goto fail;
    }
    if (has_jmprel) {
        status = apply_rela(mod, dt_jmprel, dt_pltrelsz, dt_relaent);
        if (status)
            goto fail;
    }

    spin_lock(&module_lock);
    if (find_locked(mod->name) != nullptr) {
        spin_unlock(&module_lock);
        status = EEXIST;
        goto fail;
    }
    tailq_insert_tail(&module_list, mod, link);
    listed = true;
    spin_unlock(&module_lock);

    const struct cmdline_option* const* opts_start = nullptr;
    const struct cmdline_option* const* opts_end = nullptr;

    status = ENOEXEC;
    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        const elf_phdr_t* phdr = &phdrs[i];
        const char* str;
        switch (phdr->p_type) {
        case PT_MOD_VERSION:
            if ((str = image_cstr(mod, phdr->p_vaddr, phdr->p_memsz)) != nullptr)
                mod->version = str;
            break;
        case PT_MOD_AUTHOR:
            if ((str = image_cstr(mod, phdr->p_vaddr, phdr->p_memsz)) != nullptr)
                mod->author = str;
            break;
        case PT_MOD_DESC:
            if ((str = image_cstr(mod, phdr->p_vaddr, phdr->p_memsz)) != nullptr)
                mod->desc = str;
            break;
        case PT_MOD_LICENSE:
            if ((str = image_cstr(mod, phdr->p_vaddr, phdr->p_memsz)) != nullptr)
                mod->license = str;
            break;
        case PT_MOD_CMDLINE:
            if (phdr->p_memsz < sizeof(void*))
                break;
            opts_start = image_ptr(mod, phdr->p_vaddr, phdr->p_memsz);
            if (opts_start == nullptr)
                goto fail;
            opts_end = opts_start + (phdr->p_memsz / sizeof(void*));
            break;
        case PT_MOD_EXIT: {
            if (phdr->p_memsz < sizeof(void*))
                break;
            const void* slot = image_ptr(mod, phdr->p_vaddr, sizeof(void*));
            if (slot == nullptr)
                goto fail;
            memcpy(&mod->exit, slot, sizeof(void*));
            break;
        }
        default:
            break;
        }
    }

    if (image_ptr(mod, ehdr.e_entry, 1) == nullptr)
        goto fail;

    size_t seg_idx = 0;
    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        const elf_phdr_t* phdr = &phdrs[i];
        if (phdr->p_type != PT_LOAD || phdr->p_memsz == 0)
            continue;

        const struct module_segment* seg = &mod->segments[seg_idx++];
        const enum pte_flags prot = prot_from_phdr(phdr);
        for (size_t off = 0; off < seg->size; off += page_size) {
            pmap_unmap(&kernel_vma.pmap, seg->virt + off);
            status = pmap_map(&kernel_vma.pmap, seg->virt + off, seg->phys + off, prot, CACHE_WRITE_BACK);
            if (status)
                goto fail;
        }
    }

    if (cmdline == nullptr)
        cmdline = "";
    const size_t cmdline_len = strlen(cmdline);
    mod->cmdline = mem_alloc(cmdline_len + 1, 0);
    if (mod->cmdline == nullptr) {
        status = ENOMEM;
        goto fail;
    }
    mod->cmdline_size = cmdline_len + 1;
    memcpy(mod->cmdline, cmdline, cmdline_len);
    if (opts_start != nullptr)
        cmdline_parse_buf(mod->cmdline, cmdline_len, opts_start, opts_end);

    log("Loaded module \"%s\" (%s, %s) at %p\n", mod->name, mod->desc, mod->version, (void*)mod->base);

    errno_t (*entry)(struct module*) = (errno_t (*)(struct module*))(mod->bias + ehdr.e_entry);
    status = entry(mod);
    if (status) {
        log_err("%s: Entry point returned %i\n", mod->name, status);
        goto fail;
    }

    mem_free(phdrs, ehdr.e_phnum * sizeof(elf_phdr_t));

    if (out != nullptr)
        *out = mod;
    return 0;

fail:
    if (listed) {
        spin_lock(&module_lock);
        tailq_remove(&module_list, mod, link);
        spin_unlock(&module_lock);
    }
    module_free(mod);
    mem_free(phdrs, ehdr.e_phnum * sizeof(elf_phdr_t));
    return status;
}

errno_t module_unload(struct module* mod) {
    if (mod == nullptr)
        return EINVAL;

    if (mod->exit == nullptr)
        return ENOTSUP;

    spin_lock(&module_lock);
    tailq_remove(&module_list, mod, link);
    spin_unlock(&module_lock);

    errno_t status = mod->exit(mod);
    if (status) {
        log_err("%s: Exit function returned %i\n", mod->name, status);
        spin_lock(&module_lock);
        tailq_insert_tail(&module_list, mod, link);
        spin_unlock(&module_lock);
        return status;
    }

    log("Unloaded module \"%s\"\n", mod->name);
    module_free(mod);
    return 0;
}

struct module* module_find(const char* name) {
    spin_lock(&module_lock);
    struct module* mod = find_locked(name);
    spin_unlock(&module_lock);
    return mod;
}
