// SPDX-License-Identifier: BSD-3-Clause

#include <fs/initramfs.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/clock.h>
#include <lib/cmdline.h>
#include <lib/compiler.h>
#include <lib/console.h>
#include <lib/elf.h>
#include <lib/init.h>
#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/ktest.h>
#include <lib/module.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <config.h>
#include <stdint.h>

static const char zinnia_banner[] = "Zinnia " ZINNIA_VERSION " " CONFIG_KERNEL_FLAVOR " (" ZINNIA_ARCH
                                    ", " ZINNIA_COMPILER_ID ", " ZINNIA_LINKER_ID ")";

// Path to the init executable.
static const char* init_path = "/init";
static void init_option(const char* value) {
    init_path = value;
}
CMDLINE_OPTION("init", "Path to the init executable", init_option);

static void kernel_main_task(uintptr_t arg0) {
    errno_t status = 0;
    struct boot_info* info = (struct boot_info*)arg0;

    log("Hello from main task!\n");

    // Finish the initgraph.
    initgraph_run(nullptr, info);

    // After everything is loaded, load all builtin drivers.
    module_init();

    // Load all initramfs files.
    initramfs_load_boot_modules(info);

#if CONFIG_KTEST
    ktest_main();
#endif

    const char* init_argv[] = {init_path, nullptr};
    const char* init_envp[] = {"HOME=/", "TERM=console", nullptr};

    log("Starting init \"%s\"\n", init_path);
    log("With arguments:\n");
    for (const char** arg = init_argv; *arg; arg++)
        log("    %s\n", *arg);
    log("With environment:\n");
    for (const char** env = init_envp; *env; env++)
        log("    %s\n", *env);

    status = process_spawn_init(init_path, init_argv, init_envp);
    assert(status == 0, "Failed to start init \"%s\" (%i)", init_path, status);
}

[[__init]]
void kernel_early_init() {
    percpu_bsp_init();
    percpu_get()->online = true;
    irq_lock();
}

[[noreturn]]
void kernel_main(struct boot_info* info) {
    cmdline_parse(info->cmdline);

    log("%s\n", zinnia_banner); // Say hello!
    log("Boot loader: %s\n", info->loader);
    log("Command line: %s\n", info->cmdline);

    initgraph_run(sched_stage, info);

    struct task* main_task;
    task_create("main", process_kernel(), kernel_main_task, (uintptr_t)info, &main_task);
    sched_add_task(&percpu_get()->sched, main_task);

    irq_unlock();
    sched_yield(&percpu_get()->sched);

    unreachable();
}
