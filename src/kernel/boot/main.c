#include <common/compiler.h>
#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/clock.h>
#include <kernel/cmdline.h>
#include <kernel/console.h>
#include <kernel/elf.h>
#include <kernel/init.h>
#include <kernel/irq.h>
#include <kernel/namespace.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/vmspace.h>
#include <kernel/vmo.h>
#include <config.h>
#include <stdint.h>

static const char zinnia_banner[] =
    "Zinnia " ZINNIA_VERSION " (" ZINNIA_ARCH ", " ZINNIA_COMPILER_ID ", " ZINNIA_LINKER_ID ")";

[[__init]]
void kernel_early_init() {
    percpu_bsp_init();
    percpu_get()->online = true;
    irq_lock();
}

[[noreturn]]
static void kernel_main_task(uintptr_t arg0, uintptr_t) {
    struct boot_info* info = (struct boot_info*)arg0;
    kprintf("%s\n", zinnia_banner); // Say hello!

    // TODO: Load init executable.
    const char* init_argv[] = {"init", nullptr};
    const char* init_envp[] = {"MLIBC_RTLD_DEBUG_VERBOSE=1", nullptr};

    struct vmspace* init_space;
    ASSERT(vmspace_new(&init_space) == ZN_OK, "");

    struct boot_file* file_data = &info->files[0];
    struct paged_vmo* init_file;
    ASSERT(vmo_new_phys(&init_file) == ZN_OK, "");
    vmo_write(&init_file->object, 0, HHDM_PTR(file_data->data), file_data->length, nullptr);

    struct namespace* ns;
    ASSERT(namespace_new(&ns) == ZN_OK, "");

    struct exec_info init_info = {
        .file_obj = &init_file->object,
        .ns = ns,
        .space = init_space,
        .argv = init_argv,
        .envp = init_envp,
    };

    kprintf("Loading init executable \"%s\"\n", file_data->path);

    struct task* init_task;
    zn_status_t status = elf_load(&init_info, &init_task);
    ASSERT(status == ZN_OK, "Failed to load init process (%i)\n", status);

    sched_add_task(&percpu_get()->sched, init_task);
    sched_yield(&percpu_get()->sched);

    __unreachable();
}

[[noreturn]]
void kernel_main(struct boot_info* info) {
    cmdline_parse(info->cmdline);

    console_init();
    mem_init(info->mem_map, info->num_mem_maps, info->virt_base, info->phys_base, info->hhdm_base);
    rsdp_addr = info->rsdp; // TODO
    percpu_init();
    sched_init(&percpu_get()->sched);

    struct task* main_task;
    task_create("main", &kernel_vas, nullptr, kernel_main_task, (uintptr_t)info, 0, &main_task);
    sched_add_task(&percpu_get()->sched, main_task);

    irq_unlock();
    sched_yield(&percpu_get()->sched);

    __unreachable();
}
