// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/iovec.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <syscalls/syscall.h>

static inline struct process* self() {
    return percpu_get()->sched.current->process;
}

static inline struct syscall_ret result(errno_t error, uintptr_t value) {
    return (struct syscall_ret){.value = error ? 0 : value, .error = error};
}

errno_t copy_user_path(const __user char* path, char** out);
errno_t copy_user_string(const __user char* str, size_t max, char** out);
errno_t copy_iovecs(const __user iovec_t* iov, size_t count, iovec_t* buf, iovec_iter_t* out);

bool clock_is_realtime(uintptr_t clock);

bool can_signal(struct process* sender, struct process* target, signal_t sig);
