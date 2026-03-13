#include <kernel/compiler.h>
#include <kernel/types.h>
#include <x86_64/smp.h>

extern uint8_t smp_trampoline_start[];
extern uint8_t smp_trampoline_end[];

[[noreturn]]
static void ap_entry(phys_t info) {
    // TODO

    while (1) {
        asm volatile("hlt");
    }
}

void x86_64_smp_init(uint32_t id) {}
