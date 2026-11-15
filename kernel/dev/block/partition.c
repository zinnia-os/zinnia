// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <lib/atomic.h>
#include <mem/alloc.h>
#include <stdint.h>

struct partition {
    struct blockdev blk;
    struct blockdev* parent;
    uint64_t start;
};

static _Atomic(uint32_t) next_minor;

static errno_t partition_submit(struct blockdev* blk, struct bio* bio) {
    struct partition* part = containerof(blk, struct partition, blk);
    struct blockdev* parent = part->parent;

    if (bio->op != BIO_FLUSH) {
        if (bio->lba >= blk->lba_count) {
            bio_complete(bio, bio->op == BIO_READ ? 0 : ENOSPC, 0);
            return 0;
        }
        if (bio->op == BIO_WRITE && bio->num_lbas > blk->lba_count - bio->lba) {
            bio_complete(bio, ENOSPC, 0);
            return 0;
        }
        bio->lba += part->start;
    }
    return parent->ops->submit(parent, bio);
}

static errno_t partition_ioctl(
    struct blockdev* blk,
    struct vfs_file* file,
    uintptr_t request,
    uintptr_t arg,
    uintptr_t* out
) {
    struct blockdev* parent = containerof(blk, struct partition, blk)->parent;
    if (!parent->ops->ioctl)
        return ENOTTY;
    return parent->ops->ioctl(parent, file, request, arg, out);
}

static void partition_destroy(struct blockdev* blk) {
    struct partition* part = containerof(blk, struct partition, blk);
    blockdev_unref(part->parent);
    mem_free(part, sizeof(*part));
}

static const struct blockdev_ops partition_ops = {
    .submit = partition_submit,
    .ioctl = partition_ioctl,
    .destroy = partition_destroy,
};

errno_t partition_new(struct blockdev* parent, uint64_t start_lba, uint64_t lba_count, struct blockdev** out) {
    struct partition* part = mem_alloc(sizeof(struct partition), 0);
    if (!part)
        return ENOMEM;

    blockdev_init(
        &part->blk,
        &partition_ops,
        259,
        atomic_fetch_add(&next_minor, 1, ATOMIC_RELAXED),
        parent->lba_size,
        lba_count
    );
    part->blk.limits = parent->limits;
    blockdev_ref(parent);
    part->parent = parent;
    part->start = start_lba;
    *out = &part->blk;
    return 0;
}
