// SPDX-License-Identifier: BSD-3-Clause

#include <boot/loader/loader.h>
#include <lib/elf.h>
#include <string.h>

static bool in_image(const struct kernel_image* image, uint64_t vaddr, uint64_t len) {
    return vaddr >= image->virt && len <= image->size && vaddr - image->virt <= image->size - len;
}

void* kernel_ptr(const struct kernel_image* image, uint64_t vaddr) {
    return loader_ptr(image->phys + (vaddr - image->virt));
}

static bool apply_relocations(const struct kernel_image* image, uint64_t dynamic_vaddr) {
    if (!in_image(image, dynamic_vaddr, sizeof(elf64_dyn_t)))
        return false;

    uint64_t rela = 0;
    uint64_t rela_size = 0;
    uint64_t rela_ent = sizeof(elf64_rela_t);
    for (const elf64_dyn_t* dyn = kernel_ptr(image, dynamic_vaddr); dyn->d_tag != DT_NULL; dyn++) {
        switch (dyn->d_tag) {
        case DT_RELA:
            rela = dyn->d_un.d_val;
            break;
        case DT_RELASZ:
            rela_size = dyn->d_un.d_val;
            break;
        case DT_RELAENT:
            rela_ent = dyn->d_un.d_val;
            break;
        }
    }

    if (!rela)
        return true;
    if (rela_ent < sizeof(elf64_rela_t) || !in_image(image, rela, rela_size))
        return false;

    for (uint64_t off = 0; off + rela_ent <= rela_size; off += rela_ent) {
        const elf64_rela_t* r = kernel_ptr(image, rela + off);
        const uint32_t type = ELF64_R_TYPE(r->r_info);
        if (type == ELF_R_NONE)
            continue;
        if (type != ELF_R_RELATIVE)
            return false;
        if (!in_image(image, r->r_offset, sizeof(uint64_t)))
            return false;
        *(uint64_t*)kernel_ptr(image, r->r_offset) = (uint64_t)r->r_addend;
    }

    return true;
}

bool elf_load(const uint8_t* elf, size_t size, struct kernel_image* out) {
    elf64_ehdr_t ehdr;
    if (size < sizeof(ehdr))
        return false;
    memcpy(&ehdr, elf, sizeof(ehdr));

    if (memcmp(ehdr.e_ident, ELF_MAG, sizeof(ELF_MAG)) || ehdr.e_ident[EI_CLASS] != ELF_CLASS ||
        ehdr.e_ident[EI_DATA] != ELF_DATA)
        return false;
    if (ehdr.e_machine != ELF_MACHINE)
        return false;
    if (ehdr.e_phentsize != sizeof(elf64_phdr_t) || ehdr.e_phoff > size ||
        (size - ehdr.e_phoff) / sizeof(elf64_phdr_t) < ehdr.e_phnum)
        return false;

    const elf64_phdr_t* phdrs = (const elf64_phdr_t*)(elf + ehdr.e_phoff);

    uint64_t lo = UINT64_MAX;
    uint64_t hi = 0;
    uint64_t dynamic_vaddr = 0;
    uint64_t info_vaddr = 0;
    uint64_t info_size = 0;
    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        const elf64_phdr_t* ph = &phdrs[i];
        if (ph->p_type == PT_DYNAMIC)
            dynamic_vaddr = ph->p_vaddr;
        if (ph->p_type == PT_BOOT_INFO) {
            info_vaddr = ph->p_vaddr;
            info_size = ph->p_memsz;
        }
        if (ph->p_type != PT_LOAD || ph->p_memsz == 0)
            continue;
        if (ph->p_filesz > ph->p_memsz || ph->p_offset > size || size - ph->p_offset < ph->p_filesz)
            return false;
        if (ROUND_DOWN(ph->p_vaddr, PAGE_SIZE) < lo)
            lo = ROUND_DOWN(ph->p_vaddr, PAGE_SIZE);
        if (ROUND_UP(ph->p_vaddr + ph->p_memsz, PAGE_SIZE) > hi)
            hi = ROUND_UP(ph->p_vaddr + ph->p_memsz, PAGE_SIZE);
    }
    if (lo >= hi)
        return false;

    out->virt = lo;
    out->size = hi - lo;
    out->entry = ehdr.e_entry;
    if (!in_image(out, out->entry, 1))
        return false;
    if (info_size != sizeof(struct boot_info) || !in_image(out, info_vaddr, info_size))
        return false;

    if (!loader_alloc(PHYS_STATIC, out->size, &out->phys))
        return false;

    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        const elf64_phdr_t* ph = &phdrs[i];
        if (ph->p_type != PT_LOAD || ph->p_memsz == 0)
            continue;
        memcpy(kernel_ptr(out, ph->p_vaddr), elf + ph->p_offset, ph->p_filesz);
    }
    out->info = kernel_ptr(out, info_vaddr);

    if (dynamic_vaddr)
        return apply_relocations(out, dynamic_vaddr);
    return true;
}
