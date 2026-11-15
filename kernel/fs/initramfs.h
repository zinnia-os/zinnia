// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/vfs.h>
#include <lib/errno.h>
#include <stddef.h>

struct boot_info;

bool initramfs_is_valid(const void* data, size_t len);

errno_t initramfs_load(struct vfs_path root, struct vfs_path target, const void* data, size_t len);

void initramfs_load_boot_modules(struct boot_info* info);
