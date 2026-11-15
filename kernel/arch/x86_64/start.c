// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/gdt.h>
#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/init.h>
#include <lib/percpu.h>
#include <zinnia/archctl.h>

[[__init, __naked, __used]]
void _start() {
    asm volatile(
        "mov rsp, %0\n"
        "jmp %1"
        :
        : "r"(__ld_stack_top), "r"(kernel_entry)
    );
}
