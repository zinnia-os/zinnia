// SPDX-License-Identifier: BSD-3-Clause

#pragma once

typedef __builtin_va_list va_list;

#undef va_start
#define va_start(v, ...) __builtin_va_start(v, 0)

#undef va_end
#define va_end(v) __builtin_va_end(v)

#undef va_arg
#define va_arg(v, l) __builtin_va_arg(v, l)

#undef va_copy
#define va_copy(d, s) __builtin_va_copy(d, s)
