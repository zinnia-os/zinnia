// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/ident.h>
#include <fs/vfs.h>
#include <lib/errno.h>

errno_t pipe_create(
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out_read,
    struct vfs_file** out_write
);
