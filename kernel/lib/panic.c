// SPDX-License-Identifier: BSD-3-Clause

#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/ksyms.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/unwind.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <stdarg.h>
#include <stdint.h>

#define panic_log(msg, ...) kprintf("[    !!!!    ] \e[31m" msg "\e[0m", ##__VA_ARGS__)

// ID of the CPU that is currently printing a backtrace.
static _Atomic(size_t) backtrace_cpu = SIZE_MAX;

[[__no_inline]]
static void print_backtrace(size_t cpu_id) {
    constexpr size_t max_depth = 64;

    // If the unwinder faulted on this CPU, don't try again or we'd recurse forever.
    if (atomic_load(&backtrace_cpu, ATOMIC_RELAXED) == cpu_id) {
        panic_log("Backtrace: <fault while unwinding>\n");
        return;
    }
    atomic_store(&backtrace_cpu, cpu_id, ATOMIC_RELAXED);

    panic_log("Backtrace:\n");

    struct unwind_frame frame;
    unwind_init(&frame);

    // Skip the frame of this function.
    for (size_t i = 0; i < max_depth && unwind_step(&frame); i++) {
        const uintptr_t lookup = unwind_lookup_pc(&frame);
        const char* name;
        size_t offset;
        if (ksym_lookup(lookup, &name, &offset)) {
            panic_log(
                "  #%zu [%p] %s+0x%zx%s\n",
                i,
                (void*)frame.pc,
                name,
                offset + (frame.pc - lookup),
                frame.exact_pc ? " (interrupted)" : ""
            );
        } else {
            panic_log("  #%zu [%p] <\?\?\?>%s\n", i, (void*)frame.pc, frame.exact_pc ? " (interrupted)" : "");
        }
    }

    atomic_store(&backtrace_cpu, SIZE_MAX, ATOMIC_RELAXED);
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

    print_backtrace(cpu->id);
    panic_log("End of panic message\n");

    initgraph_panic();

    while (true) {
        irq_wait();
    }
}
