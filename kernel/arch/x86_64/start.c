// SPDX-License-Identifier: BSD-3-Clause

#include <lib/compiler.h>

DEFINE_ASM_FUNC_SECTION(
    _start,
    ".init.text",
    "lea rsp, [rip + __ld_stack_top]\n"
    "jmp kernel_entry\n"
);
