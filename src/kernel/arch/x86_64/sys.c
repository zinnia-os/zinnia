#include <zinnia/archctl.h>
#include <zinnia/status.h>
#include <common/compiler.h>
#include <kernel/init.h>
#include <kernel/percpu.h>
#include <x86_64/apic.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>
#include <x86_64/gdt.h>

[[__init, __naked]]
void _start() {
    asm volatile(
        "lea rsp, [rip + %0]\n"
        "jmp %1"
        :
        : "i"(__ld_stack_top), "r"(kernel_entry)
    );
}

void arch_panic() {}

zn_status_t arch_archctl(zn_archctl_t op, uintptr_t arg) {
    switch (op) {
    case ZN_ARCHCTL_SET_FSBASE:
        asm_wrmsr(MSR_FS_BASE, arg);
        return 0;
    default:
        return ZN_ERR_INVALID;
    }
}
