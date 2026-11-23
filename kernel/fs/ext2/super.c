// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ext2: " fmt

#include <fs/ext2/ext2.h>
#include <lib/module.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <mem/alloc.h>

static errno_t ext2_super_sync(struct vfs_super_block* base) {
    // TODO: Write back inodes and file system metadata.
    return ENOSYS;
}

static errno_t ext2_super_statvfs(struct vfs_super_block* base, struct __statvfs* out) {
    // TODO: Report block and inode counts.
    return ENOSYS;
}

static void ext2_super_destroy(struct vfs_super_block* base) {
    struct ext2_super* sb = ext2_sb(base);

    // TODO: Write back the clean state and purge cached blocks.
    blockdev_release(sb->dev);
    blockdev_unref(sb->dev);
    mem_free(sb, sizeof(*sb));
}

static const struct vfs_super_block_ops ext2_super_ops = {
    .sync = ext2_super_sync,
    .statvfs = ext2_super_statvfs,
    .destroy = ext2_super_destroy,
};

static errno_t ext2_mount(const struct vfs_filesystem* fs, enum mount_flags flags, void* arg, struct vfs_mount** out) {
    if (!arg)
        return EINVAL;

    // Open the block device at path `arg`.
    struct process* proc = percpu_get()->sched.current->process;

    mutex_lock(&proc->lock);
    struct vfs_path root = vfs_path_clone(proc->root);
    struct vfs_path cwd = vfs_path_clone(proc->cwd);
    mutex_unlock(&proc->lock);

    struct vfs_path target;
    errno_t status =
        vfs_path_lookup(root, cwd, arg, &proc->identity, VFS_LOOKUP_MUST_EXIST | VFS_LOOKUP_FOLLOW_SYMLINKS, &target);
    vfs_path_drop(root);
    vfs_path_drop(cwd);
    if (status)
        return status;

    struct vfs_node* node = vfs_entry_get_inode(target.entry);
    vfs_path_drop(target);
    if (!node)
        return ENOENT;

    struct blockdev* dev;
    status = blockdev_from_node(node, &dev);
    vfs_node_unref(node);
    if (status)
        return status;

    status = blockdev_claim(dev);
    if (status) {
        blockdev_unref(dev);
        return status;
    }

    struct ext2_super* sb = mem_alloc(sizeof(*sb), 0);
    if (!sb) {
        blockdev_release(dev);
        blockdev_unref(dev);
        return ENOMEM;
    }
    refcount_init(&sb->base.rc);
    sb->base.ops = &ext2_super_ops;
    sb->base.flags = flags;
    sb->dev = dev;

    // TODO: Read the super block and group descriptors.
    // TODO: Load the root inode and create the mount.
    status = ENOSYS;

    vfs_super_block_unref(&sb->base);
    return status;
}

static const struct vfs_filesystem ext2_fs = {
    .name = "ext2",
    .mount = ext2_mount,
};

static errno_t ext2_init(struct module* module) {
    return vfs_register_filesystem(&ext2_fs);
}

MODULE_ENTRY(ext2_init);
MODULE_DESC("ext2 file system");
MODULE_AUTHOR("Marvin Friedrich");
MODULE_LICENSE("BSD-3-Clause");
MODULE_VERSION("1.0");
