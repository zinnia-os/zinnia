// SPDX-License-Identifier: BSD-3-Clause

#include <dev/char/chardev.h>
#include <fs/ident.h>
#include <fs/vfs.h>
#include <lib/assert.h>

static struct vfs_mount* dev_mount;

struct vfs_path devtmpfs_root() {
    return vfs_path_clone((struct vfs_path){.mount = dev_mount, .entry = dev_mount->root});
}

static errno_t devtmpfs_mount(
    const struct vfs_filesystem* fs,
    enum mount_flags flags,
    void* arg,
    struct vfs_mount** out
) {
    return vfs_mount_new(flags, dev_mount->root, out);
}

static const struct vfs_filesystem devtmpfs = {
    .name = "devtmpfs",
    .mount = devtmpfs_mount,
};

static void devtmpfs_stage_init(struct boot_info* info) {
    errno_t status = vfs_mount("tmpfs", 0, nullptr, &dev_mount);
    assert(status == 0, "Unable to create the devtmpfs (%i)", status);

    status = vfs_register_filesystem(&devtmpfs);
    assert(status == 0, "Unable to register the devtmpfs (%i)", status);
}

INIT_TASK(devtmpfs_stage, "fs.devtmpfs", devtmpfs_stage_init);
INIT_DEPENDS(devtmpfs_stage, vfs_stage);

static void devtmpfs_mount_stage_init(struct boot_info* info) {
    // Create /dev.
    struct vfs_path root = vfs_get_root();
    errno_t status = vfs_mkdir(root, root, "/dev", 0755, ident_kernel());
    assert(status == 0 || status == EEXIST, "Unable to create /dev (%i)", status);

    // Get /dev.
    struct vfs_path target;
    status = vfs_path_lookup(root, root, "/dev", ident_kernel(), VFS_LOOKUP_MUST_EXIST, &target);
    vfs_path_drop(root);
    assert(status == 0, "Unable to look up /dev (%i)", status);

    // Prepare a devtmpfs for mounting.
    struct vfs_mount* mount;
    status = vfs_mount("devtmpfs", 0, nullptr, &mount);
    assert(status == 0, "Unable to mount the devtmpfs (%i)", status);

    // Mount it on /dev.
    status = vfs_path_mount(target, mount);
    assert(status == 0, "Unable to mount the devtmpfs on /dev (%i)", status);
    vfs_mount_unref(mount);
    vfs_path_drop(target);
}

INIT_TASK(devtmpfs_mount_stage, "fs.devtmpfs.mount", devtmpfs_mount_stage_init);
INIT_DEPENDS(devtmpfs_mount_stage, devtmpfs_stage);
