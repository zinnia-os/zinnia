#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/elf.h>
#include <kernel/exec.h>
#include <kernel/percpu.h>
#include <kernel/pmap.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/utils.h>
#include <kernel/vfs.h>
#include <kernel/vm_object.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>
#include <uapi/limits.h>
#include <uapi/mman.h>
#include <string.h>

struct elf_info {
    uintptr_t at_phdr;
    size_t at_phnum;
    size_t at_phent;
    uintptr_t at_entry;
};

static errno_t elf_load_file(
    struct file* file,
    struct process* proc,
    struct exec_info* info,
    uintptr_t base,
    struct elf_info* out_info
) {
    errno_t status;
    ssize_t read;
    struct elf_ehdr ehdr = {};
    status = file_read(file, &ehdr, sizeof(ehdr), 0, &read);
    if (status)
        return status;

    const size_t page_size = arch_mem_page_size();
    const uintptr_t base_addr = ehdr.e_type == ET_DYN ? base : 0;

    for (size_t i = 0; i < ehdr.e_phnum; i++) {
        struct elf_phdr phdr = {};
        file_read(file, &phdr, sizeof(phdr), (ehdr.e_phoff + (i * ehdr.e_phentsize)), &read);
        if (phdr.p_type == PT_LOAD) {
            enum prot_flags prot = 0;
            if (phdr.p_flags & PF_R)
                prot |= PROT_READ;
            if (phdr.p_flags & PF_W)
                prot |= PROT_WRITE;
            if (phdr.p_flags & PF_X)
                prot |= PROT_EXEC;

            if (phdr.p_offset % phdr.p_align != phdr.p_vaddr % phdr.p_align)
                return ENOEXEC;

            // Copy the file data into its own mapping.
            status =
                file_mmap(file, info->space, base_addr + phdr.p_vaddr, phdr.p_memsz, prot, MAP_PRIVATE, phdr.p_offset);
            if (status)
                return status;
        } else if (phdr.p_type == PT_INTERP) {
            const size_t interp_len = MIN(phdr.p_filesz, PATH_MAX);
            char* interp_buf = mem_alloc(interp_len, 0);
            if (!interp_buf)
                return ENOMEM;

            ssize_t read;
            status = file_read(file, interp_buf, interp_len, phdr.p_offset, &read);
            if (status)
                return status;
            if ((size_t)read != interp_len)
                return ENOEXEC;
        }
    }

    out_info->at_entry = ehdr.e_entry;
    out_info->at_phdr = base + ehdr.e_phoff;
    out_info->at_phent = ehdr.e_phentsize;
    out_info->at_phnum = ehdr.e_phnum;

    return 0;
}

static errno_t elf_load(
    struct exec_format* format,
    struct process* proc,
    struct exec_info* info,
    struct task** result
) {
    const size_t page_size = arch_mem_page_size();

    struct elf_info elf_info;
    errno_t status = elf_load_file(info->executable, proc, info, 0x10000, &elf_info);
    if (status)
        return status;

    // Load an interpreter if one was requested and override the entry point.
    uintptr_t entry = elf_info.at_entry;
    if (info->interpreter != nullptr) {
        struct elf_info interp_info;
        status = elf_load_file(info->interpreter, proc, info, ((uintptr_t)1 << (mem_high_shift() - 1)), &interp_info);
        if (status)
            return status;

        entry = interp_info.at_entry;
    }

    const uintptr_t highest = ((uintptr_t)1 << (mem_high_shift() - 1)) - page_size;
    const size_t stack_size = 2 * 1024 * 1024; // 2MiB stack by default.
    const uintptr_t stack_start = highest - stack_size;

    // Allocate stack.
    struct paged_vmo* stack;
    status = vm_object_new_phys(&stack);
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
        vm_object_write(&stack->object, stack_off, &nul, sizeof(nul), nullptr);

        const size_t len = strlen(info->envp[env]);
        stack_off -= len;
        vm_object_write(&stack->object, stack_off, info->envp[env], len, nullptr);

        envp_offsets[env] = stack_start + stack_off;
    }

    for (size_t arg = 0; arg < num_argv; arg++) {
        const char nul = 0;
        stack_off -= 1;
        vm_object_write(&stack->object, stack_off, &nul, sizeof(nul), nullptr);

        const size_t len = strlen(info->argv[arg]);
        stack_off -= len;
        vm_object_write(&stack->object, stack_off, info->argv[arg], len, nullptr);

        argv_offsets[arg] = stack_start + stack_off;
    }

    stack_off = ALIGN_DOWN(stack_off, 16);

    // Align the stack if argc + argv + envp does not add up to 16 byte alignment.
    if ((1 + num_argv + num_envp) & 1) {
        stack_off -= sizeof(uintptr_t);
        uintptr_t zero = 0;
        vm_object_write(&stack->object, stack_off, &zero, sizeof(zero), nullptr);
    }

#define WRITE_AUXV(auxv, value) \
    do { \
        stack_off -= sizeof(uintptr_t); \
        uintptr_t auxv_val = value; \
        vm_object_write(&stack->object, stack_off, &auxv_val, sizeof(uintptr_t), nullptr); \
        stack_off -= sizeof(uintptr_t); \
        auxv_val = auxv; \
        vm_object_write(&stack->object, stack_off, &auxv_val, sizeof(uintptr_t), nullptr); \
    } while (0)

    // Write auxiliary values.
    WRITE_AUXV(AT_NULL, 0);   // Terminator, always last.
    WRITE_AUXV(AT_SECURE, 0); // Never secure
    WRITE_AUXV(AT_PHDR, elf_info.at_phdr);
    WRITE_AUXV(AT_PHNUM, elf_info.at_phnum);
    WRITE_AUXV(AT_PHENT, elf_info.at_phent);
    WRITE_AUXV(AT_ENTRY, elf_info.at_entry);

    // envp pointers.
    stack_off -= sizeof(uintptr_t);
    const uintptr_t zero = 0;
    vm_object_write(&stack->object, stack_off, &zero, sizeof(zero), nullptr);
    for (size_t env = 0; env < num_envp; env++) {
        stack_off -= sizeof(uintptr_t);
        vm_object_write(&stack->object, stack_off, &envp_offsets[env], sizeof(envp_offsets[env]), nullptr);
    }

    // argv pointers.
    stack_off -= sizeof(uintptr_t);
    vm_object_write(&stack->object, stack_off, &zero, sizeof(zero), nullptr);
    for (size_t arg = 0; arg < num_argv; arg++) {
        stack_off -= sizeof(uintptr_t);
        vm_object_write(&stack->object, stack_off, &argv_offsets[arg], sizeof(argv_offsets[arg]), nullptr);
    }

    mem_free(envp_offsets);
    mem_free(argv_offsets);

    // argc
    stack_off -= sizeof(uintptr_t);
    uintptr_t argc = num_argv;
    vm_object_write(&stack->object, stack_off, &argc, sizeof(argc), nullptr);

    status = vm_space_map(info->space, &stack->object, stack_start, stack_size, PROT_READ | PROT_WRITE, 0);
    if (status)
        return status;

    struct task_start_info* start_info = mem_alloc(sizeof(struct task_start_info), 0);
    start_info->ip = entry;
    start_info->sp = highest - stack_size + stack_off;
    start_info->arg = 0;

    struct task* new_task;
    status = task_create(info->argv[0], proc, sched_to_user, (uintptr_t)start_info, &new_task);
    if (status)
        return status;

    *result = new_task;
    return 0;
}

static bool elf_identify(struct exec_format* format, struct file* file) {
    struct elf_ehdr ehdr;
    ssize_t read;
    errno_t status = file_read(file, &ehdr, sizeof(ehdr), 0, &read);
    if (status)
        return false;
    if (read != sizeof(ehdr))
        return false;

    // Check if the file is an ELF for this machine.
    if (memcmp(ehdr.e_ident, ELF_MAG, sizeof(ELF_MAG)) != 0)
        return false;
    if (ehdr.e_ident[EI_VERSION] != EV_CURRENT)
        return false;
    if (ehdr.e_ident[EI_CLASS] != ELF_ARCH_CLASS)
        return false;
    if (ehdr.e_ident[EI_DATA] != ELF_ARCH_DATA)
        return false;
    if (ehdr.e_machine != ELF_ARCH_MACHINE)
        return false;
    if (ehdr.e_type != ET_EXEC && ehdr.e_type != ET_DYN)
        return false;

    return true;
}

static const struct exec_format elf_format = {
    .identify = elf_identify,
    .load = elf_load,
};

[[__init]]
void elf_init() {
    exec_register("elf", &elf_format);
}
