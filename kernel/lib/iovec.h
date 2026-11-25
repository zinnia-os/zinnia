// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/utils.h>
#include <mem/usercopy.h>
#include <stddef.h>
#include <zinnia/limits.h>
#include <zinnia/uio.h>

typedef struct __iovec iovec_t;

constexpr size_t IOV_MAX = __IOV_MAX;

typedef struct {
    void* iov_base;
    size_t iov_len;
} kvec_t;

typedef struct {
    union {
        iovec_t* iovecs;
        kvec_t* kvecs;
    };
    size_t iovecs_len;
    size_t total_len;
    size_t total_offset;
    size_t current_idx;
    size_t current_offset;
    // Whether the iovecs point to user memory.
    bool user;
} iovec_iter_t;

static inline size_t iovec_iter_len(iovec_iter_t* iter) {
    return iter->total_len - iter->total_offset;
}

// Creates an iovec iterator over kernel memory.
iovec_iter_t iovec_iter_kernel(kvec_t* kvecs, size_t count);

// Creates an iovec iterator over user memory.
iovec_iter_t iovec_iter_user(iovec_t* iovecs, size_t count);

// Copies up to `len` bytes from `src` into the iterator and advances it.
// Returns the amount of bytes actually copied in `out_copied`.
errno_t iovec_iter_copy_to(iovec_iter_t* iter, const void* src, size_t len, size_t* out_copied);

// Copies up to `len` bytes out of the iterator into `dst` and advances it.
// Returns the amount of bytes actually copied in `out_copied`.
errno_t iovec_iter_copy_from(iovec_iter_t* iter, void* dst, size_t len, size_t* out_copied);
