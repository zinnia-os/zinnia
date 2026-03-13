#include <kernel/irq.h>
#include <kernel/panic.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <stdarg.h>

[[noreturn]]
void panic(const char* msg, ...) {
    // TODO: Stop all other CPUs with IPI.
    arch_irq_set_state(false);

    struct percpu* cpu = percpu_get();
    struct task* current = cpu->sched.current;
    kprintf(
        "----[ Kernel panic ]----\n"
        "In task \"%s\" (TID %zu) on CPU %zu!\n",
        current ? current->name : nullptr,
        current ? current->id : 0,
        cpu->id
    );

    va_list args;
    va_start(args, msg);
    kvprintf(msg, args);
    va_end(args);

    kprintf("----[ End of panic ]----\n");

    while (1) {
        arch_irq_wait();
    }
}
