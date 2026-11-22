// SPDX-License-Identifier: BSD-3-Clause

#include <fs/exec/exec.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/elf.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/layout.h>
#include <mem/mman.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <stdckdint.h>
#include <zinnia/limits.h>

constexpr uintptr_t ELF_DYN_BASE = 0x10000;

struct elf_image {
    uintptr_t base;
    uintptr_t entry;
    uintptr_t phdr;
    size_t phnum;
    size_t phent;
};

static bool elf_identify(struct vmo* obj) {
    elf_ehdr_t ehdr = {};
    size_t read;
    if (vmo_read(obj, 0, &ehdr, sizeof(ehdr), &read) || read != sizeof(ehdr))
        return false;

    return memcmp(ehdr.e_ident, ELF_MAG, sizeof(ELF_MAG)) == 0;
}

static errno_t elf_read_interp(struct vmo* obj, const elf_phdr_t* phdr, char** out) {
    if (phdr->p_filesz < 2 || phdr->p_filesz > __PATH_MAX)
        return ENOEXEC;

    char* path = mem_alloc(__PATH_MAX, ALLOC_NOZERO);
    if (!path)
        return ENOMEM;

    size_t read;
    errno_t err = vmo_read(obj, phdr->p_offset, path, phdr->p_filesz, &read);
    if (!err && (read != phdr->p_filesz || path[phdr->p_filesz - 1] != '\0'))
        err = ENOEXEC;
    if (err) {
        mem_free(path, __PATH_MAX);
        return err;
    }
    *out = path;
    return 0;
}

static errno_t elf_map(
    struct vma* space,
    struct vmo* obj,
    uintptr_t dyn_base,
    struct elf_image* out,
    char** out_interp
) {
    errno_t err;

    size_t read;
    elf_ehdr_t ehdr = {};
    err = vmo_read(obj, 0, &ehdr, sizeof(ehdr), &read);
    if (err)
        return err;
    if (read != sizeof(ehdr))
        return ENOEXEC;

    // Check if the file is an ELF.
    if (memcmp(ehdr.e_ident, ELF_MAG, sizeof(ELF_MAG)) != 0)
        return EINVAL;
    if (ehdr.e_ident[EI_VERSION] != EV_CURRENT)
        return EINVAL;
    if (ehdr.e_ident[EI_CLASS] != ELF_CLASS)
        return EINVAL;
    if (ehdr.e_ident[EI_DATA] != ELF_DATA)
        return EINVAL;
    if (ehdr.e_machine != ELF_MACHINE)
        return EINVAL;
    if (ehdr.e_type != ET_EXEC && ehdr.e_type != ET_DYN)
        return EINVAL;

    const uintptr_t base = ehdr.e_type == ET_DYN ? dyn_base : 0;
    uintptr_t entry;
    if (ckd_add(&entry, ehdr.e_entry, base) || !mem_addr_is_user(entry))
        return ENOEXEC;

    const size_t phdrs_size = (size_t)ehdr.e_phnum * ehdr.e_phentsize;
    uintptr_t phdrs_end;
    if (ehdr.e_phentsize < sizeof(elf_phdr_t) || ehdr.e_phnum == 0 || phdrs_size > 64 * 1024)
        return ENOEXEC;
    if (ckd_add(&phdrs_end, ehdr.e_phoff, phdrs_size))
        return ENOEXEC;

    const size_t pgsz = page_size();
    uintptr_t phdr_addr = 0;

    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        elf_phdr_t phdr = {};
        err = vmo_read(obj, (ehdr.e_phoff + (i * ehdr.e_phentsize)), &phdr, sizeof(phdr), &read);
        if (err)
            return err;
        if (read != sizeof(phdr))
            return ENOEXEC;

        if (phdr.p_type == PT_PHDR)
            phdr_addr = phdr.p_vaddr + base;

        if (phdr.p_type == PT_INTERP && out_interp) {
            if (*out_interp)
                return ENOEXEC;
            err = elf_read_interp(obj, &phdr, out_interp);
            if (err)
                return err;
        }

        if (phdr.p_type == PT_LOAD) {
            if (phdr.p_memsz == 0)
                continue;
            if (phdr.p_filesz > phdr.p_memsz)
                return ENOEXEC;

            uintptr_t vaddr, seg_end;
            if (ckd_add(&vaddr, phdr.p_vaddr, base) || ckd_add(&seg_end, vaddr, phdr.p_memsz))
                return ENOEXEC;
            if (!mem_addr_is_user(vaddr) || !mem_addr_is_user(seg_end - 1))
                return ENOEXEC;

            if (phdr.p_align > 1 && phdr.p_offset % phdr.p_align != phdr.p_vaddr % phdr.p_align)
                return ENOEXEC;

            if (phdr_addr == 0 && ehdr.e_phoff >= phdr.p_offset && phdrs_end - phdr.p_offset <= phdr.p_filesz)
                phdr_addr = vaddr + (ehdr.e_phoff - phdr.p_offset);

            enum prot_flags prot = 0;
            if (phdr.p_flags & PF_R)
                prot |= PROT_READ;
            if (phdr.p_flags & PF_W)
                prot |= PROT_WRITE;
            if (phdr.p_flags & PF_X)
                prot |= PROT_EXEC;

            const uintptr_t misalign = vaddr & (pgsz - 1);
            const uintptr_t map_address = vaddr - misalign;
            const size_t backed_map_size = (phdr.p_filesz + misalign + pgsz - 1) & ~(pgsz - 1);
            const size_t total_map_size = (phdr.p_memsz + misalign + pgsz - 1) & ~(pgsz - 1);

            if (backed_map_size) {
                err = vma_map(space, obj, map_address, backed_map_size, prot, 0, phdr.p_offset - misalign);
                if (err)
                    return err;
            }
            if (total_map_size > backed_map_size) {
                err = vma_map(
                    space,
                    nullptr,
                    map_address + backed_map_size,
                    total_map_size - backed_map_size,
                    prot,
                    0,
                    0
                );
                if (err)
                    return err;
            }

            const uintptr_t file_end = vaddr + phdr.p_filesz;
            if (phdr.p_memsz > phdr.p_filesz && file_end % pgsz != 0) {
                const uintptr_t zero_end = MIN(ROUND_UP(file_end, pgsz), seg_end);
                err = vma_zero(space, file_end, zero_end - file_end);
                if (err)
                    return err;
            }
        }
    }

    *out = (struct elf_image){
        .base = base,
        .entry = entry,
        .phdr = phdr_addr,
        .phnum = ehdr.e_phnum,
        .phent = ehdr.e_phentsize,
    };
    return 0;
}

static errno_t elf_load(struct process* proc, struct exec_info* info, uintptr_t* out_ip, uintptr_t* out_sp) {
    struct elf_image exe = {};
    char* interp_path = nullptr;
    errno_t err = elf_map(info->space, info->file_obj, ELF_DYN_BASE, &exe, &interp_path);

    uintptr_t entry = exe.entry;
    uintptr_t interp_base = 0;
    if (!err && interp_path) {
        struct vmo* interp;
        err = exec_open(proc, interp_path, &interp);
        if (!err) {
            struct elf_image ld;
            if (!elf_identify(interp))
                err = ELIBBAD;
            else
                err = elf_map(info->space, interp, (uintptr_t)1 << (pmap_high_shift() - 2), &ld, nullptr);
            if (!err) {
                entry = ld.entry;
                interp_base = ld.base;
            }
            vmo_unref(interp);
        }
    }
    mem_free(interp_path, __PATH_MAX);
    if (err)
        return err;

    const size_t pgsz = page_size();

    const uintptr_t highest = ((uintptr_t)1 << (pmap_high_shift() - 1)) - pgsz;
    const size_t stack_size = 2 * 1024 * 1024; // 2MiB
    const uintptr_t stack_start = highest - stack_size;

    err = vma_map(info->space, nullptr, stack_start, stack_size, PROT_READ | PROT_WRITE, 0, 0);
    if (err)
        return err;

    // Fill in stack info.
    uintptr_t stack_off = stack_size;

    uintptr_t execfn = 0;
    if (info->exec_path) {
        const char nul = 0;
        stack_off -= 1;
        vma_write(info->space, stack_start + stack_off, &nul, sizeof(nul));

        const size_t len = strlen(info->exec_path);
        stack_off -= len;
        vma_write(info->space, stack_start + stack_off, info->exec_path, len);
        execfn = stack_start + stack_off;
    }

    // TODO: Instead of allocating, calculate it on the fly.
    size_t num_envp = 0;
    for (const char** ptr = info->envp; *ptr != nullptr; ptr++) {
        num_envp++;
    }
    size_t num_argv = 0;
    for (const char** ptr = info->argv; *ptr != nullptr; ptr++) {
        num_argv++;
    }

    size_t strings_size = 0;
    for (size_t env = 0; env < num_envp; env++)
        strings_size += strlen(info->envp[env]) + 1;
    for (size_t arg = 0; arg < num_argv; arg++)
        strings_size += strlen(info->argv[arg]) + 1;

    uintptr_t* envp_offsets = mem_alloc(num_envp * sizeof(uintptr_t), 0);
    uintptr_t* argv_offsets = mem_alloc(num_argv * sizeof(uintptr_t), 0);
    if ((num_envp && !envp_offsets) || (num_argv && !argv_offsets)) {
        mem_free(envp_offsets, num_envp * sizeof(uintptr_t));
        mem_free(argv_offsets, num_argv * sizeof(uintptr_t));
        return ENOMEM;
    }

    for (size_t env = 0; env < num_envp; env++) {
        const char nul = 0;
        stack_off -= 1;
        vma_write(info->space, stack_start + stack_off, &nul, sizeof(nul));

        const size_t len = strlen(info->envp[env]);
        stack_off -= len;
        vma_write(info->space, stack_start + stack_off, info->envp[env], len);

        envp_offsets[env] = stack_start + stack_off;
    }

    for (size_t arg = 0; arg < num_argv; arg++) {
        const char nul = 0;
        stack_off -= 1;
        vma_write(info->space, stack_start + stack_off, &nul, sizeof(nul));

        const size_t len = strlen(info->argv[arg]);
        stack_off -= len;
        vma_write(info->space, stack_start + stack_off, info->argv[arg], len);

        argv_offsets[arg] = stack_start + stack_off;
    }

    stack_off = ROUND_DOWN(stack_off, 16);

    // Align the stack if argc + argv + envp does not add up to 16 byte alignment.
    if ((1 + num_argv + num_envp) & 1) {
        stack_off -= sizeof(uintptr_t);
        uintptr_t zero = 0;
        vma_write(info->space, stack_start + stack_off, &zero, sizeof(zero));
    }

#define WRITE_AUXV(auxv, value) \
    do { \
        stack_off -= sizeof(uintptr_t); \
        uintptr_t auxv_val = value; \
        vma_write(info->space, stack_start + stack_off, &auxv_val, sizeof(uintptr_t)); \
        stack_off -= sizeof(uintptr_t); \
        auxv_val = auxv; \
        vma_write(info->space, stack_start + stack_off, &auxv_val, sizeof(uintptr_t)); \
    } while (0)

    // Write auxiliary values.
    WRITE_AUXV(AT_NULL, 0);
    WRITE_AUXV(AT_SECURE, 0);
    WRITE_AUXV(AT_PAGESZ, pgsz);
    WRITE_AUXV(AT_PHNUM, exe.phnum);
    WRITE_AUXV(AT_PHENT, exe.phent);
    WRITE_AUXV(AT_ENTRY, exe.entry);
    WRITE_AUXV(AT_MINSIGSTKSZ, signal_frame_size());
    if (exe.phdr)
        WRITE_AUXV(AT_PHDR, exe.phdr);
    if (interp_base)
        WRITE_AUXV(AT_BASE, interp_base);
    if (execfn)
        WRITE_AUXV(AT_EXECFN, execfn);

    // envp pointers.
    stack_off -= sizeof(uintptr_t);
    const uintptr_t zero = 0;
    vma_write(info->space, stack_start + stack_off, &zero, sizeof(zero));
    for (size_t env = num_envp; env > 0; env--) {
        stack_off -= sizeof(uintptr_t);
        vma_write(info->space, stack_start + stack_off, &envp_offsets[env - 1], sizeof(envp_offsets[env - 1]));
    }

    // argv pointers.
    stack_off -= sizeof(uintptr_t);
    vma_write(info->space, stack_start + stack_off, &zero, sizeof(zero));
    for (size_t arg = num_argv; arg > 0; arg--) {
        stack_off -= sizeof(uintptr_t);
        vma_write(info->space, stack_start + stack_off, &argv_offsets[arg - 1], sizeof(argv_offsets[arg - 1]));
    }

    mem_free(envp_offsets, num_envp * sizeof(uintptr_t));
    mem_free(argv_offsets, num_argv * sizeof(uintptr_t));

    // argc
    stack_off -= sizeof(uintptr_t);
    uintptr_t argc = num_argv;
    vma_write(info->space, stack_start + stack_off, &argc, sizeof(argc));

    *out_ip = entry;
    *out_sp = highest - stack_size + stack_off;
    return 0;
}

static struct exec_format elf_format = {
    .name = "elf",
    .identify = elf_identify,
    .load = elf_load,
};

static void elf_stage_init(struct boot_info*) {
    exec_register(&elf_format);
}
INIT_TASK(elf_stage, "fs.exec.elf", elf_stage_init);
INIT_DEPENDS(elf_stage, mem_stage);
INIT_ENTAILS(elf_stage, vfs_stage);
