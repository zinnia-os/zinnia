// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/vfs.h>

// Sets the global root of the VFS. Takes a new reference to `root`.
void vfs_set_root(struct vfs_path root);

// Returns true if both paths reference the same entry on the same mount.
static inline bool vfs_path_equal(struct vfs_path a, struct vfs_path b) {
    return a.mount == b.mount && a.entry == b.entry;
}

void pipe_destroy(struct pipe* pipe);
errno_t vfs_fifo_open(struct vfs_node* node, struct vfs_file* file);
