// SPDX-License-Identifier: BSD-3-Clause

#pragma once

// Attributes that affect data layout.
#define __section(x) gnu::section(x)
#define __aligned(x) gnu::aligned(x)
#define __packed     gnu::packed
#define __may_alias  gnu::may_alias
#define __naked      gnu::naked

// These hints may improve code generation.
#define __cold              gnu::cold
#define __hot               gnu::hot
#define __used              gnu::used
#define __unused            gnu::unused
#define __format(like, ...) gnu::format(like, __VA_ARGS__)
#define __force_inline      gnu::always_inline
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
