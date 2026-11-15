// SPDX-License-Identifier: BSD-3-Clause

#pragma once
#include <lib/compiler.h>

// Stop all execution upon panic.
[[noreturn, __format(printf, 1, 2)]]
void panic(const char* msg, ...);
