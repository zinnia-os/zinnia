#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/clock.h>
#include <kernel/cmdline.h>
#include <kernel/compiler.h>
#include <kernel/console.h>
#include <kernel/elf.h>
#include <kernel/futex.h>
#include <kernel/init.h>
#include <kernel/irq.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/vm_object.h>
#include <kernel/vm_space.h>
#include <config.h>
#include <stdint.h>

static const char zinnia_banner[] =
    "Zinnia " ZINNIA_VERSION " (" ZINNIA_ARCH ", " ZINNIA_COMPILER_ID ", " ZINNIA_LINKER_ID ")";

static void kernel_main_task(uintptr_t arg0) {
    struct boot_info* info = (struct boot_info*)arg0;
    kprintf("%s\n", zinnia_banner); // Say hello!

    ASSERT(info->num_files >= 1, "No init executable provided\n");
}

[[noreturn]]
void kernel_main(struct boot_info* info) {
    cmdline_parse(info->cmdline);

    console_init();
    mem_init(info->mem_map, info->num_mem_maps, info->virt_base, info->phys_base, info->hhdm_base);
    rsdp_addr = info->rsdp; // TODO
    percpu_init();
    sched_init(&percpu_get()->sched);
    futex_init();

    struct task* main_task;
    task_create("main", &kernel_space, kernel_main_task, (uintptr_t)info, &main_task);
    sched_add_task(&percpu_get()->sched, main_task);

    irq_unlock();
    sched_yield(&percpu_get()->sched);

    __unreachable();
}

[[__init]]
void kernel_early_init() {
    percpu_bsp_init();
    percpu_get()->online = true;
    irq_lock();
}
