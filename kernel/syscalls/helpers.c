// SPDX-License-Identifier: BSD-3-Clause

#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/iovec.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>
#include <stdckdint.h>
#include <stddef.h>
#include <stdint.h>
#include <zinnia/limits.h>

errno_t copy_user_string(const __user char* str, size_t max, char** out) {
    size_t len;
    if (!usercopy_strlen(str, max, &len))
        return EFAULT;
    if (len >= max)
        return ENAMETOOLONG;

    char* buf = mem_alloc(max, 0);
    if (!buf)
        return ENOMEM;
    if (!usercopy_read(buf, str, len)) {
        mem_free(buf, max);
        return EFAULT;
    }
    buf[len] = 0;
    *out = buf;
    return 0;
}

errno_t copy_user_path(const __user char* path, char** out) {
    if (!path)
        return EFAULT;

    size_t len;
    if (!usercopy_strlen(path, __PATH_MAX, &len))
        return EFAULT;
    if (len >= __PATH_MAX)
        return ENAMETOOLONG;

    char* kpath = mem_alloc(__PATH_MAX, ALLOC_NOZERO);
    if (!kpath)
        return ENOMEM;
    if (!usercopy_read(kpath, path, len)) {
        mem_free(kpath, __PATH_MAX);
        return EFAULT;
    }
    kpath[len] = 0;

    *out = kpath;
    return 0;
}

errno_t copy_iovecs(const __user iovec_t* iov, size_t count, iovec_t* buf, iovec_iter_t* out) {
    if (count > IOV_MAX)
        return EINVAL;
    if (count && !usercopy_read(buf, iov, count * sizeof(iovec_t)))
        return EFAULT;

    size_t total = 0;
    for (size_t i = 0; i < count; i++) {
        if (ckd_add(&total, total, buf[i].iov_len) || total > SIZE_MAX / 2)
            return EINVAL;
    }

    *out = iovec_iter_user(buf, count);
    return 0;
}
