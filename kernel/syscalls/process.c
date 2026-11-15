// SPDX-License-Identifier: BSD-3-Clause

#include <lib/panic.h>
#include <syscalls/private.h>
#include <syscalls/syscall.h>

struct syscall_ret sys_exit(int err) {
    struct syscall_ret ret = {};

    ret.error = ENOSYS;

    panic("sys::exit() not implemented!\n");

    return ret;
}

struct syscall_ret sys_umask(mode_t mask) {
    struct syscall_ret ret = {};

    struct process* proc = self();
    mutex_lock(&proc->lock);
    ret.value = proc->umask;
    proc->umask = mask & 0777;
    mutex_unlock(&proc->lock);
    return ret;
}
