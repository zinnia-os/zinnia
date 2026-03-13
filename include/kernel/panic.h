#pragma once

#include <kernel/compiler.h>

// Stop all execution upon panic.
[[noreturn, __format(printf, 1, 2)]]
void panic(const char* msg, ...);
