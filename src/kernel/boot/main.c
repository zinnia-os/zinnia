#include <common/compiler.h>
#include <kernel/assert.h>
#include <kernel/cmdline.h>
#include <kernel/console.h>
#include <kernel/init.h>
#include <kernel/irq.h>
#include <kernel/mem.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <config.h>

static const char zinnia_banner[] =
    "Zinnia " ZINNIA_VERSION " (" ZINNIA_ARCH ", " ZINNIA_COMPILER_ID ", " ZINNIA_LINKER_ID ")";

[[__init]]
void kernel_early_init() {
    percpu_bsp_init();
    percpu_get()->online = true;
    irq_lock();
}

[[noreturn]]
void kernel_main(struct boot_info* info) {
    cmdline_parse(info->cmdline);
    console_init();

    kprintf("%s\n", zinnia_banner); // Say hello!
    kprintf("Command line: \"%s\"\n", info->cmdline);

    mem_init(info->mem_map, info->num_mem_maps, info->virt_base, info->phys_base, info->hhdm_base);

    percpu_init();
    sched_init();

    irq_unlock();
    sched_reschedule(&percpu_get()->sched);
    while (1) {}
    ASSERT(false, "Nothing to do!");
}

[[noreturn]]
void kernel_main_task() {
    while (1) {}

    ASSERT(false, "Nothing to do");
}
