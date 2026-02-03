#include <menix/archctl.h>
#include <menix/status.h>
#include <kernel/compiler.h>
#include <kernel/init.h>
#include <kernel/percpu.h>
#include "asm.h"
#include "defs.h"

[[__init, __naked]]
void _start() {
    asm volatile(
        "lea rsp, [rip + %0]\n"
        "jmp %1"
        :
        : "i"(__ld_stack_top), "r"(kernel_entry)
    );
}

void percpu_bsp_early_init() {
    asm_wrmsr(MSR_GS_BASE, (uint64_t)&percpu_bsp);
    asm_wrmsr(MSR_FS_BASE, 0);
    asm_wrmsr(MSR_KERNEL_GS_BASE, 0);
}

[[noreturn]]
void arch_panic() {
    asm volatile("cli; hlt");
    __unreachable();
}

menix_status_t arch_archctl(menix_archctl_t op, uintptr_t arg) {
    switch (op) {
    case MENIX_ARCHCTL_SET_FSBASE:
        asm_wrmsr(MSR_FS_BASE, arg);
        return 0;
    default:
        return MENIX_ERR_BAD_ARG;
    }
}
