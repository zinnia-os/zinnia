// SPDX-License-Identifier: BSD-3-Clause

#include <fs/private.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/initgraph.h>
#include <mem/alloc.h>

static void vfs_stage_init(struct boot_info* info) {
    // Mount a tmpfs as root.
    struct vfs_mount* tmpfs;
    errno_t status = vfs_mount("tmpfs", 0, nullptr, &tmpfs);
    assert(status == 0, "Unable to mount the root tmpfs (%i)", status);

    struct vfs_path root = {.mount = tmpfs, .entry = tmpfs->root};
    vfs_set_root(root);
    vfs_mount_unref(tmpfs);
}

INIT_TASK(vfs_stage, "fs.vfs", vfs_stage_init);
INIT_DEPENDS(vfs_stage, mem_stage);
