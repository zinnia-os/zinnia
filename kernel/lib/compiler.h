// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/utils.h>

// Attributes that affect data layout.
#define __section(x) gnu::section(x)
#define __aligned(x) gnu::aligned(x)
#define __packed     gnu::packed
#define __may_alias  gnu::may_alias

// These hints may improve code generation.
#define __cold              gnu::cold
#define __hot               gnu::hot
#define __used              gnu::used
#define __unused            gnu::unused
#define __format(like, ...) gnu::format(like, __VA_ARGS__)
#define __force_inline      gnu::always_inline
#define __no_inline         gnu::noinline
#define __visibility        gnu::visibility
#define __pure              gnu::pure

// Only clang has these hints, GCC would throw a warning here.
// clang::noderef also doesn't support the C23 attribute format.
#ifdef __clang__
#define __user __attribute__((noderef, address_space(1)))
#define __phys __attribute__((noderef, address_space(2)))
#else
#define __user
#define __phys
#endif

#define __likely(x)   __builtin_expect(!!(x), 1)
#define __unlikely(x) __builtin_expect(!!(x), 0)

// Defines global inline assembly.
// Some compilers don't allow using input parameters with `asm` directly, so
// this macro creates an empty function and puts regular assembly inside.
#define DEFINE_ASM(section, flags, body, ...) \
    [[__used]] \
    static void UNIQUE_IDENT(__asm_)() { \
        asm volatile(".pushsection " section ", \"" flags "\", @progbits\n" body \
                     ".popsection\n" __VA_OPT__(::__VA_ARGS__)); \
    } \
    static_assert(true)

#define DEFINE_ASM_FUNC_SECTION(name, section, body, ...) \
    DEFINE_ASM( \
        section, \
        "ax", \
        ".global " #name "\n.type " #name ", @function\n" #name ":\n.cfi_startproc\n" body \
        ".cfi_endproc\n.size " #name ", . - " #name "\n" __VA_OPT__(, ) __VA_ARGS__ \
    )

#define DEFINE_ASM_FUNC(name, body, ...) DEFINE_ASM_FUNC_SECTION(name, ".text." #name, body __VA_OPT__(, ) __VA_ARGS__)
