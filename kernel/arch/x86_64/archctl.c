// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <lib/errno.h>
#include <mem/layout.h>
#include <syscalls/syscall.h>
#include <bits/archctl.h>
#include <zinnia/archctl.h>

struct syscall_ret arch_archctl(uintptr_t op, uintptr_t arg) {
    struct syscall_ret ret = {};

    switch (op) {
    case __ARCHCTL_SET_FSBASE:
        if (!arch_mem_addr_is_user(arg)) {
            ret.error = EINVAL;
            return ret;
        }
        asm_wrmsr(MSR_FS_BASE, arg);
        return ret;
    case __ARCHCTL_SET_GSBASE:
        if (!arch_mem_addr_is_user(arg)) {
            ret.error = EINVAL;
            return ret;
        }
        // Update the inactive GSBASE MSR, not ours.
        asm_wrmsr(MSR_KERNEL_GS_BASE, arg);
        return ret;
    default:
        ret.error = EINVAL;
        return ret;
    }
}
