// SPDX-License-Identifier: BSD-3-Clause

#include <lib/iovec.h>
#include <lib/utils.h>
#include <mem/usercopy.h>
#include <string.h>

// Copies between `buf` and the iterator. If `to_iter` is set, data flows from `buf` into the iovecs.
static errno_t iovec_iter_copy(iovec_iter_t* iter, void* buf, size_t len, bool to_iter, size_t* out_copied) {
    size_t progress = 0;
    errno_t status = 0;

    while (progress < len && iter->current_idx < iter->iovecs_len) {
        const size_t vec_len =
            iter->user ? iter->iovecs[iter->current_idx].iov_len : iter->kvecs[iter->current_idx].iov_len;
        size_t remaining = vec_len - iter->current_offset;
        if (remaining == 0) {
            iter->current_idx++;
            iter->current_offset = 0;
            continue;
        }

        size_t chunk = MIN(remaining, len - progress);
        uint8_t* kbuf = (uint8_t*)buf + progress;

        if (iter->user) {
            __user uint8_t* uptr = (__user uint8_t*)iter->iovecs[iter->current_idx].iov_base + iter->current_offset;
            bool ok = to_iter ? usercopy_write(uptr, kbuf, chunk) : usercopy_read(kbuf, uptr, chunk);
            if (!ok) {
                status = EFAULT;
                break;
            }
        } else {
            uint8_t* ptr = (uint8_t*)iter->kvecs[iter->current_idx].iov_base + iter->current_offset;
            if (to_iter)
                memcpy(ptr, kbuf, chunk);
            else
                memcpy(kbuf, ptr, chunk);
        }

        progress += chunk;
        iter->current_offset += chunk;
        iter->total_offset += chunk;
    }

    if (out_copied)
        *out_copied = progress;
    return status;
}

iovec_iter_t iovec_iter_kernel(kvec_t* kvecs, size_t count) {
    size_t total = 0;
    for (size_t i = 0; i < count; i++)
        total += kvecs[i].iov_len;

    return (iovec_iter_t){
        .kvecs = kvecs,
        .iovecs_len = count,
        .total_len = total,
        .total_offset = 0,
        .current_idx = 0,
        .current_offset = 0,
        .user = false,
    };
}

iovec_iter_t iovec_iter_user(iovec_t* iovecs, size_t count) {
    size_t total = 0;
    for (size_t i = 0; i < count; i++)
        total += iovecs[i].iov_len;

    return (iovec_iter_t){
        .iovecs = iovecs,
        .iovecs_len = count,
        .total_len = total,
        .total_offset = 0,
        .current_idx = 0,
        .current_offset = 0,
        .user = true,
    };
}

errno_t iovec_iter_copy_to(iovec_iter_t* iter, const void* src, size_t len, size_t* out_copied) {
    return iovec_iter_copy(iter, (void*)src, len, true, out_copied);
}

errno_t iovec_iter_copy_from(iovec_iter_t* iter, void* dst, size_t len, size_t* out_copied) {
    return iovec_iter_copy(iter, dst, len, false, out_copied);
}
