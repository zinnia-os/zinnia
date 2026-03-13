#include <kernel/archctl.h>
#include <kernel/compiler.h>
#include <kernel/init.h>
#include <kernel/percpu.h>
#include <uapi/errno.h>
#include <x86_64/apic.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>
#include <x86_64/gdt.h>

[[__init, __naked, __used]]
void _start() {
    asm volatile(
        "lea rsp, [rip + %0]\n"
        "jmp %1"
        :
        : "i"(__ld_stack_top), "r"(kernel_entry)
    );
}

void arch_panic() {
    // TODO
}

errno_t arch_archctl(enum archctl_op op, uintptr_t arg) {
    switch (op) {
    case ARCHCTL_SET_FSBASE:
        asm_wrmsr(MSR_FS_BASE, arg);
        return 0;
    default:
        return EINVAL;
    }
}
