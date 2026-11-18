// SPDX-License-Identifier: BSD-3-Clause

#include <boot/efi/efi.h>
#include <lib/elf.h>
#include <stddef.h>
#include <stdint.h>

efi_status efistub_relocate(uintptr_t base, const elf64_dyn_t* dynamic) {
    uintptr_t rela = 0;
    size_t rela_size = 0;
    size_t rela_ent = sizeof(elf64_rela_t);
    uintptr_t relr = 0;
    size_t relr_size = 0;

    for (const elf64_dyn_t* dyn = dynamic; dyn->d_tag != DT_NULL; dyn++) {
        switch (dyn->d_tag) {
        case DT_RELA:
            rela = base + dyn->d_un.d_val;
            break;
        case DT_RELASZ:
            rela_size = dyn->d_un.d_val;
            break;
        case DT_RELAENT:
            rela_ent = dyn->d_un.d_val;
            break;
        case DT_RELR:
            relr = base + dyn->d_un.d_val;
            break;
        case DT_RELRSZ:
            relr_size = dyn->d_un.d_val;
            break;
        }
    }

    for (size_t off = 0; rela && off + rela_ent <= rela_size; off += rela_ent) {
        const elf64_rela_t* r = (const elf64_rela_t*)(rela + off);
        const uint32_t type = ELF64_R_TYPE(r->r_info);
        if (type == ELF_R_NONE)
            continue;
        if (type != ELF_R_RELATIVE)
            return EFI_LOAD_ERROR;
        *(uint64_t*)(base + r->r_offset) = base + r->r_addend;
    }

    uint64_t* where = nullptr;
    for (size_t off = 0; relr && off + sizeof(uint64_t) <= relr_size; off += sizeof(uint64_t)) {
        uint64_t entry = *(const uint64_t*)(relr + off);
        if (!(entry & 1)) {
            where = (uint64_t*)(base + entry);
            *where++ += base;
            continue;
        }
        for (size_t i = 0; (entry >>= 1) != 0; i++) {
            if (entry & 1)
                where[i] += base;
        }
        where += 63;
    }

    return EFI_SUCCESS;
}
