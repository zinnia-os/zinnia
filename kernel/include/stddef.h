// SPDX-License-Identifier: BSD-3-Clause

#pragma once

typedef __SIZE_TYPE__ size_t;
typedef __PTRDIFF_TYPE__ ptrdiff_t;

typedef __WCHAR_TYPE__ wchar_t;
typedef typeof(nullptr) nullptr_t;

#undef offsetof
#define offsetof(s, m) __builtin_offsetof(s, m)

#undef unreachable
#define unreachable() __builtin_unreachable()
