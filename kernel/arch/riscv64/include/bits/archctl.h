// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <syscalls/syscall.h>
#include <stdint.h>

struct syscall_ret arch_archctl(uintptr_t op, uintptr_t value);
