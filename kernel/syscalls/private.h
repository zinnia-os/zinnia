// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <syscalls/syscall.h>

static inline struct process* self() {
    return percpu_get()->sched.current->process;
}

errno_t copy_user_path(const __user char* path, char** out);
errno_t copy_user_string(const __user char* str, size_t max, char** out);
