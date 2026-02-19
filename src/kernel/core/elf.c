#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <common/elf.h>
#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/elf.h>
#include <kernel/mmu.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/vas.h>
#include <kernel/vdso.h>
#include <kernel/vmo.h>
#include <string.h>

zn_status_t elf_load(struct exec_info* info, struct task** out) {
    zn_status_t status;

    size_t read;
    struct elf_ehdr ehdr = {};
    vmo_read(info->file_obj, 0, &ehdr, sizeof(ehdr), &read);

    // Check if the file is an ELF.
    if (memcmp(ehdr.e_ident, ELF_MAG, sizeof(ELF_MAG)) != 0)
        return ZN_ERR_INVALID;
    if (ehdr.e_ident[EI_VERSION] != EV_CURRENT)
        return ZN_ERR_INVALID;
    if (ehdr.e_ident[EI_CLASS] != ELF_ARCH_CLASS)
        return ZN_ERR_INVALID;
    if (ehdr.e_ident[EI_DATA] != ELF_ARCH_DATA)
        return ZN_ERR_INVALID;
    if (ehdr.e_machine != ELF_ARCH_MACHINE)
        return ZN_ERR_INVALID;
    if (ehdr.e_type != ET_EXEC)
        return ZN_ERR_INVALID;

    const size_t page_size = arch_mem_page_size();

    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        struct elf_phdr phdr = {};
        vmo_read(info->file_obj, (ehdr.e_phoff + (i * ehdr.e_phentsize)), &phdr, sizeof(phdr), &read);
        if (phdr.p_type == PT_LOAD) {
            enum zn_vm_flags prot = 0;
            if (phdr.p_flags & PF_R)
                prot |= ZN_VM_MAP_READ;
            if (phdr.p_flags & PF_W)
                prot |= ZN_VM_MAP_WRITE;
            if (phdr.p_flags & PF_X)
                prot |= ZN_VM_MAP_EXEC;

            ASSERT(phdr.p_offset % phdr.p_align == phdr.p_vaddr % phdr.p_align, "");

            const uintptr_t misalign = phdr.p_vaddr & (page_size - 1);
            const uintptr_t map_address = phdr.p_vaddr - misalign;
            const size_t backed_map_size = (phdr.p_filesz + misalign + page_size - 1) & ~(page_size - 1);
            const size_t total_map_size = (phdr.p_memsz + misalign + page_size - 1) & ~(page_size - 1);

            // Copy the file data into its own mapping.
            struct paged_vmo* phdr_obj;
            status = vmo_new_phys(&phdr_obj);
            if (status)
                return status;

            status = vmo_copy(&phdr_obj->object, phdr.p_offset, info->file_obj, phdr.p_offset, phdr.p_filesz, nullptr);
            if (status)
                return status;

            // We map more than we copied so the rest is filled with zeroed pages.
            status = vas_map_vmo(info->space, &phdr_obj->object, phdr.p_vaddr, phdr.p_memsz, prot, phdr.p_offset);
            if (status)
                return status;
        }
    }

    const uintptr_t highest = ((uintptr_t)1 << (mem_high_shift() - 1)) - page_size;
    const size_t stack_size = 2 * 1024 * 1024; // 2MiB
    const uintptr_t stack_start = highest - stack_size;

    // Allocate stack.
    struct paged_vmo* stack;
    status = vmo_new_phys(&stack);
    if (status)
        return status;

    // Fill in stack info.
    uintptr_t stack_off = stack_size;

    // TODO: Instead of allocating, calculate it on the fly.
    size_t num_envp = 0;
    for (const char** ptr = info->envp; *ptr != nullptr; ptr++) {
        num_envp++;
    }
    size_t num_argv = 0;
    for (const char** ptr = info->argv; *ptr != nullptr; ptr++) {
        num_argv++;
    }
    uintptr_t* envp_offsets = mem_alloc(num_envp * sizeof(uintptr_t), 0);
    uintptr_t* argv_offsets = mem_alloc(num_argv * sizeof(uintptr_t), 0);

    for (size_t env = 0; env < num_envp; env++) {
        const char nul = 0;
        stack_off -= 1;
        vmo_write(&stack->object, stack_off, &nul, sizeof(nul), nullptr);

        const size_t len = strlen(info->envp[env]);
        stack_off -= len;
        vmo_write(&stack->object, stack_off, info->envp[env], len, nullptr);

        envp_offsets[env] = stack_start + stack_off;
    }

    for (size_t arg = 0; arg < num_argv; arg++) {
        const char nul = 0;
        stack_off -= 1;
        vmo_write(&stack->object, stack_off, &nul, sizeof(nul), nullptr);

        const size_t len = strlen(info->argv[arg]);
        stack_off -= len;
        vmo_write(&stack->object, stack_off, info->argv[arg], len, nullptr);

        argv_offsets[arg] = stack_start + stack_off;
    }

    stack_off = ALIGN_DOWN(stack_off, 16);

    // Align the stack if argc + argv + envp does not add up to 16 byte alignment.
    if ((1 + num_argv + num_envp) & 1) {
        stack_off -= sizeof(uintptr_t);
        uintptr_t zero = 0;
        vmo_write(&stack->object, stack_off, &zero, sizeof(zero), nullptr);
    }

    uintptr_t auxv_val = 0;

    // TODO: Use a VMO that's shared between all tasks.
    size_t vdso_len = __ld_vdso_end - __ld_vdso_start;
    struct paged_vmo* vdso;
    status = vmo_new_phys(&vdso);
    if (status)
        return status;

    vmo_write(&vdso->object, 0, __ld_vdso_start, vdso_len, nullptr);

    const uintptr_t vdso_addr = ((uintptr_t)1 << (mem_high_shift() - 2));
    vas_map_vmo(info->space, &vdso->object, vdso_addr, vdso_len, ZN_VM_MAP_READ | ZN_VM_MAP_EXEC, 0);

#define WRITE_AUXV(auxv, value) \
    stack_off -= sizeof(uintptr_t); \
    auxv_val = value; \
    vmo_write(&stack->object, stack_off, &auxv_val, sizeof(uintptr_t), nullptr); \
    stack_off -= sizeof(uintptr_t); \
    auxv_val = auxv; \
    vmo_write(&stack->object, stack_off, &auxv_val, sizeof(uintptr_t), nullptr)

    // Write auxiliary values.
    WRITE_AUXV(AT_NULL, 0);
    WRITE_AUXV(AT_SECURE, 0);
    WRITE_AUXV(AT_PHNUM, ehdr.e_phnum);
    WRITE_AUXV(AT_PHENT, ehdr.e_phentsize);
    WRITE_AUXV(AT_ENTRY, ehdr.e_entry);
    WRITE_AUXV(AT_SYSINFO_EHDR, vdso_addr);

    // envp pointers.
    stack_off -= sizeof(uintptr_t);
    const uintptr_t zero = 0;
    vmo_write(&stack->object, stack_off, &zero, sizeof(zero), nullptr);
    for (size_t env = 0; env < num_envp; env++) {
        stack_off -= sizeof(uintptr_t);
        vmo_write(&stack->object, stack_off, &envp_offsets[env], sizeof(envp_offsets[env]), nullptr);
    }

    // argv pointers.
    stack_off -= sizeof(uintptr_t);
    vmo_write(&stack->object, stack_off, &zero, sizeof(zero), nullptr);
    for (size_t arg = 0; arg < num_argv; arg++) {
        stack_off -= sizeof(uintptr_t);
        vmo_write(&stack->object, stack_off, &argv_offsets[arg], sizeof(argv_offsets[arg]), nullptr);
    }

    mem_free(envp_offsets);
    mem_free(argv_offsets);

    // argc
    stack_off -= sizeof(uintptr_t);
    uintptr_t argc = num_argv;
    vmo_write(&stack->object, stack_off, &argc, sizeof(argc), nullptr);

    status = vas_map_vmo(info->space, &stack->object, stack_start, stack_size, ZN_VM_MAP_READ | ZN_VM_MAP_WRITE, 0);
    if (status)
        return status;

    struct task* result;
    status = task_create(
        info->argv[0],
        info->space,
        info->ns,
        sched_to_user,
        ehdr.e_entry,
        highest - stack_size + stack_off,
        &result
    );
    if (status)
        return status;

    *out = result;
    return ZN_OK;
}
