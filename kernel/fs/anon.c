// SPDX-License-Identifier: BSD-3-Clause

// anonfs is a virtual file system. It backs files that don't live in any other
// file system, like pipes or unbound sockets.

#include <fs/vfs.h>
#include <lib/atomic.h>

static const struct vfs_super_block_ops anon_super_ops = {};

static struct vfs_super_block anon_super = {
    .rc = {.count = 1}, // There's always just this exact super block.
    .ops = &anon_super_ops,
};

static const struct vfs_node_ops anon_node_ops = {};

static _Atomic(ino_t) anon_inode_counter = 1;

errno_t vfs_create_anon_inode(
    const struct vfs_file_ops* file_ops,
    enum vfs_node_type type,
    mode_t mode,
    const struct ident* identity,
    struct vfs_node** out
) {
    struct vfs_node* node;
    errno_t status = vfs_node_new(&anon_super, type, &anon_node_ops, mode, &node);
    if (status)
        return status;

    node->file_ops = file_ops;
    // Every anon node gets its own inode counter.
    node->id = atomic_fetch_add(&anon_inode_counter, 1, ATOMIC_RELAXED);
    node->uid = identity->euid;
    node->gid = identity->egid;

    *out = node;
    return 0;
}
