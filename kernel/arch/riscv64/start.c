// SPDX-License-Identifier: BSD-3-Clause

#include <lib/compiler.h>
#include <lib/init.h>

DEFINE_ASM_FUNC_SECTION(
    _start,
    ".init.text",
    "la sp, __ld_stack_top\n"
    "j %[entry]\n"
    "unimp\n",
    [entry] "i"(kernel_entry)
);
