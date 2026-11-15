// SPDX-License-Identifier: BSD-3-Clause

#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/ksyms.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <stdarg.h>
#include <stdint.h>

#define panic_log(msg, ...) kprintf("[    !!!!    ] \e[31m" msg "\e[0m", ##__VA_ARGS__)

static void print_backtrace() {
    constexpr size_t max_depth = 32;
    void** fp = __builtin_frame_address(0);

    panic_log("Backtrace:\n");
    for (size_t i = 0; i < max_depth; i++) {
        if (!fp || (uintptr_t)fp < mem_hhdm_base || (uintptr_t)fp % alignof(void*))
            break;

        void* ret = fp[1];
        if (!ret)
            break;

        const char* name;
        size_t offset;
        // The return address may already point past the end of a noreturn call.
        if (ksym_lookup((uintptr_t)ret - 1, &name, &offset))
            panic_log("  #%zu [%p] %s+0x%zx\n", i, ret, name, offset + 1);
        else
            panic_log("  #%zu [%p] <\?\?\?>\n", i, ret);

        void** next = fp[0];
        if (next <= fp)
            break;
        fp = next;
    }
}

[[noreturn]]
void panic(const char* msg, ...) {
    // TODO: Stop all other CPUs with IPI.
    irq_set_state(false);

    struct percpu* cpu = percpu_get();
    struct task* current = cpu->sched.current;
    kprintf(
        "[    !!!!    ] \e[31m"
        "Kernel panic on CPU %zu (T%zu, \"%s\"):\e[0m\n",
        cpu->id,
        current ? current->id : 0,
        current ? current->name : nullptr
    );

    kprintf("[    !!!!    ] \e[31m");

    va_list args;
    va_start(args, msg);
    kvprintf(msg, args);
    va_end(args);

    kprintf("\e[0m");

    print_backtrace();
    panic_log("End of panic message\n");

    initgraph_panic();

    while (true) {
        irq_wait();
    }
}
