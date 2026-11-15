// SPDX-License-Identifier: BSD-3-Clause

#include <dev/char/chardev.h>
#include <dev/char/devctl.h>

void chardev_init(
    struct chardev* dev,
    const struct chardev_ops* ops,
    const struct vfs_file_ops* file_ops,
    uint32_t major,
    uint32_t minor
) {
    refcount_init(&dev->rc);
    dev->ops = ops;
    dev->file_ops = file_ops;
    dev->dev = dev_make(major, minor);
}

void chardev_ref(struct chardev* dev) {
    refcount_inc(&dev->rc);
}

void chardev_unref(struct chardev* dev) {
    if (refcount_dec(&dev->rc) && dev->ops && dev->ops->destroy)
        dev->ops->destroy(dev);
}

static errno_t chardev_node_open(struct vfs_node* node, struct vfs_file* file) {
    struct chardev* dev = node->priv;
    file->ops = dev->file_ops;
    if (dev->ops && dev->ops->open)
        return dev->ops->open(dev, file);
    return 0;
}

static void chardev_node_destroy(struct vfs_node* node) {
    chardev_unref(node->priv);
}

static const struct vfs_node_ops chardev_node_ops = {
    .destroy = chardev_node_destroy,
    .device = {
        .open = chardev_node_open,
    },
};

errno_t devtmpfs_add_chardev(const char* name, struct chardev* dev, mode_t mode) {
    struct vfs_mknod_target target = {
        .type = VNODE_CHARDEV,
        .ops = &chardev_node_ops,
        .priv = dev,
        .rdev = dev->dev,
    };

    chardev_ref(dev);
    struct vfs_path root = devtmpfs_root();
    errno_t status = vfs_mknod(root, root, name, mode, &target, ident_kernel(), nullptr);
    vfs_path_drop(root);
    if (status) {
        chardev_unref(dev);
        return status;
    }
    devctl_notify_create(name);
    return 0;
}

errno_t devtmpfs_mkdir(const char* name, mode_t mode) {
    struct vfs_path root = devtmpfs_root();
    errno_t status = vfs_mkdir(root, root, name, mode, ident_kernel());
    vfs_path_drop(root);
    return status;
}

errno_t devtmpfs_remove(const char* name) {
    struct vfs_path root = devtmpfs_root();
    errno_t status = vfs_unlink(root, root, name, ident_kernel());
    vfs_path_drop(root);
    if (status)
        return status;
    devctl_notify_destroy(name);
    return 0;
}

errno_t devtmpfs_symlink(const char* name, const char* target) {
    struct vfs_path root = devtmpfs_root();
    errno_t status = vfs_symlink(root, root, name, target, ident_kernel());
    vfs_path_drop(root);
    return status;
}
