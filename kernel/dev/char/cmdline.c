// SPDX-License-Identifier: BSD-3-Clause

#include <dev/char/chardev.h>
#include <lib/assert.h>
#include <lib/cmdline.h>
#include <string.h>

static errno_t cmdline_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    const char* cmdline = cmdline_get();
    const size_t len = strlen(cmdline);
    if (offset >= len) {
        *out_read = 0;
        return 0;
    }

    size_t copied;
    errno_t status = iovec_iter_copy_to(buffer, cmdline + offset, len - offset, &copied);
    *out_read = copied;
    return status;
}

static const struct vfs_file_ops cmdline_file_ops = {
    .read = cmdline_read,
};

static struct chardev cmdline_dev;

static void cmdline_stage_init(struct boot_info* info) {
    chardev_init(&cmdline_dev, nullptr, &cmdline_file_ops, 1, 12);
    errno_t status = devtmpfs_add_chardev("cmdline", &cmdline_dev, 0666);
    assert(status == 0, "Unable to create /dev/cmdline (%i)", status);
}

INIT_TASK(cmdline_stage, "dev.char.cmdline", cmdline_stage_init);
INIT_DEPENDS(cmdline_stage, devtmpfs_stage);
