// SPDX-License-Identifier: BSD-3-Clause

#include <lib/percpu.h>
#include <mem/layout.h>
#include <mem/usercopy.h>

static bool is_user_range(uintptr_t addr, size_t len) {
    if (len == 0)
        return mem_addr_is_user(addr);
    return addr + len > addr && mem_addr_is_user(addr) && mem_addr_is_user(addr + len - 1);
}

bool usercopy_read(void* dst, const __user void* src, size_t len) {
    if (!is_user_range((uintptr_t)src, len))
        return false;

    arch_usercopy_allow(true);
    bool success = arch_usercopy_read(dst, src, len, &percpu_get()->sched.current->usercopy_region);
    arch_usercopy_allow(false);
    return success;
}

bool usercopy_write(__user void* dst, const void* src, size_t len) {
    if (!is_user_range((uintptr_t)dst, len))
        return false;

    arch_usercopy_allow(true);
    bool success = arch_usercopy_write(dst, src, len, &percpu_get()->sched.current->usercopy_region);
    arch_usercopy_allow(false);
    return success;
}

bool usercopy_strlen(const __user char* str, size_t max, size_t* len) {
    if (!mem_addr_is_user((uintptr_t)str))
        return false;

    size_t unused;
    if (len == nullptr)
        len = &unused;

    arch_usercopy_allow(true);
    bool success = arch_usercopy_strlen(str, max, len, &percpu_get()->sched.current->usercopy_region);
    arch_usercopy_allow(false);
    return success;
}
