// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/errno.h>
#include <stddef.h>
#include <stdint.h>

struct vma;

errno_t futex_wait(struct vma* space, uintptr_t addr, int expected, const uint64_t* deadline_ns);
size_t futex_wake(struct vma* space, uintptr_t addr, size_t count);
