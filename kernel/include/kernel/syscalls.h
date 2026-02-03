#pragma once

#include <menix/status.h>
#include <kernel/compiler.h>
#include <stddef.h>
#include <stdint.h>

[[noreturn]]
void syscall_panic(menix_status_t err);

menix_status_t syscall_log(__user const char* msg, size_t len);

menix_status_t syscall_archctl(uint32_t op, size_t value);
