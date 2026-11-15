// SPDX-License-Identifier: BSD-3-Clause

#include <dev/char/chardev.h>
#include <lib/assert.h>
#include <lib/random.h>
#include <lib/utils.h>

constexpr size_t MEMFILE_CHUNK = 64;

static const uint8_t zero_chunk[MEMFILE_CHUNK];

static errno_t memfile_discard(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    *out_written = iovec_iter_len(buffer);
    return 0;
}

static errno_t memfile_read_zero(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    size_t total = 0;
    errno_t status = 0;
    while (iovec_iter_len(buffer)) {
        size_t copied;
        status = iovec_iter_copy_to(buffer, zero_chunk, MIN(iovec_iter_len(buffer), MEMFILE_CHUNK), &copied);
        total += copied;
        if (status)
            break;
    }
    *out_read = total;
    return status;
}

static errno_t null_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    *out_read = 0;
    return 0;
}

static const struct vfs_file_ops null_file_ops = {
    .read = null_read,
    .write = memfile_discard,
};

static const struct vfs_file_ops zero_file_ops = {
    .read = memfile_read_zero,
    .write = memfile_discard,
};

static errno_t full_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    *out_written = 0;
    return ENOSPC;
}

static const struct vfs_file_ops full_file_ops = {
    .read = memfile_read_zero,
    .write = full_write,
};

static errno_t random_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    uint8_t chunk[MEMFILE_CHUNK];
    size_t total = 0;
    errno_t status = 0;
    while (iovec_iter_len(buffer)) {
        const size_t len = MIN(iovec_iter_len(buffer), sizeof(chunk));

        // TODO: This function sucks and is probably not useful for cryptography.
        random_fill(chunk, len);

        size_t copied;
        status = iovec_iter_copy_to(buffer, chunk, len, &copied);
        total += copied;
        if (status)
            break;
    }
    *out_read = total;
    return status;
}

static const struct vfs_file_ops random_file_ops = {
    .read = random_read,
    .write = memfile_discard,
};

static struct chardev null_dev;
static struct chardev zero_dev;
static struct chardev full_dev;
static struct chardev random_dev;
static struct chardev urandom_dev;

static void memfiles_add(const char* name, struct chardev* dev, const struct vfs_file_ops* ops, uint32_t minor) {
    chardev_init(dev, nullptr, ops, 1, minor);
    errno_t status = devtmpfs_add_chardev(name, dev, 0666);
    assert(status == 0, "Unable to create /dev/%s (%i)", name, status);
}

static void memfiles_stage_init(struct boot_info* info) {
    memfiles_add("null", &null_dev, &null_file_ops, 3);
    memfiles_add("zero", &zero_dev, &zero_file_ops, 5);
    memfiles_add("full", &full_dev, &full_file_ops, 7);
    memfiles_add("random", &random_dev, &random_file_ops, 8);
    memfiles_add("urandom", &urandom_dev, &random_file_ops, 9);
}

INIT_TASK(memfiles_stage, "dev.char.memfiles", memfiles_stage_init);
INIT_DEPENDS(memfiles_stage, devtmpfs_stage, random_stage);
