#pragma once

#include <kernel/compiler.h>
#include <stdarg.h>

[[__format(printf, 1, 2)]]
void kprintf(const char* message, ...);

void kvprintf(const char* message, va_list args);
