// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/vfs.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/refcount.h>
#include <lib/types.h>
#include <stdint.h>

static inline dev_t dev_make(uint32_t major, uint32_t minor) {
    return (minor & 0xFF) | ((major & 0xFFF) << 8) | ((dev_t)(minor & ~0xFFU) << 12) | ((dev_t)(major & ~0xFFFU) << 32);
}

struct chardev;

struct chardev_ops {
    errno_t (*open)(struct chardev* dev, struct vfs_file* file);
    void (*destroy)(struct chardev* dev);
};

struct chardev {
    struct refcount rc;
    const struct chardev_ops* ops;
    const struct vfs_file_ops* file_ops;
    dev_t dev;
};

void chardev_init(
    struct chardev* dev,
    const struct chardev_ops* ops,
    const struct vfs_file_ops* file_ops,
    uint32_t major,
    uint32_t minor
);
void chardev_ref(struct chardev* dev);
void chardev_unref(struct chardev* dev);

struct vfs_path devtmpfs_root();
errno_t devtmpfs_add_chardev(const char* name, struct chardev* dev, mode_t mode);
errno_t devtmpfs_mkdir(const char* name, mode_t mode);
errno_t devtmpfs_remove(const char* name);
errno_t devtmpfs_symlink(const char* name, const char* target);

INIT_EXTERN(devtmpfs_stage);
