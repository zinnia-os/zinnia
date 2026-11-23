// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/ident.h>
#include <fs/vfs.h>
#include <lib/errno.h>
#include <stdint.h>

errno_t eventfd_create(
    uint64_t initval,
    bool semaphore,
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out
);
