// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <lib/atomic.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <string.h>

constexpr uint32_t RAM_MAJOR = 1;
constexpr size_t RAM_LBA_SIZE = 512;

struct ramdisk {
    struct blockdev blk;
    phys_t base;
};

static atomic_uint next_minor;

static errno_t ramdisk_submit(struct blockdev* blk, struct bio* bio) {
    struct ramdisk* ram = containerof(blk, struct ramdisk, blk);

    if (bio->op == BIO_FLUSH) {
        bio_complete(bio, 0, 0);
        return 0;
    }
    if (bio->lba >= blk->lba_count || bio->num_lbas > blk->lba_count - bio->lba) {
        bio_complete(bio, bio->op == BIO_READ ? 0 : ENOSPC, 0);
        return 0;
    }

    uint64_t lba = bio->lba;
    for (size_t i = 0; i < bio->num_segs; i++) {
        const struct bio_seg* seg = &bio->segs[i];
        void* store = HHDM_PTR(ram->base + lba * RAM_LBA_SIZE);
        void* buffer = HHDM_PTR(seg->addr);
        if (bio->op == BIO_READ)
            memcpy(buffer, store, seg->len);
        else
            memcpy(store, buffer, seg->len);
        lba += seg->len / RAM_LBA_SIZE;
    }

    bio_complete(bio, 0, bio->num_lbas);
    return 0;
}

static void ramdisk_destroy(struct blockdev* blk) {
    mem_free(containerof(blk, struct ramdisk, blk), sizeof(struct ramdisk));
}

static const struct blockdev_ops ramdisk_ops = {
    .submit = ramdisk_submit,
    .destroy = ramdisk_destroy,
};

errno_t ramdisk_new(phys_t base, size_t length, struct blockdev** out) {
    struct ramdisk* ram = mem_alloc(sizeof(struct ramdisk), 0);
    if (!ram)
        return ENOMEM;

    blockdev_init(
        &ram->blk,
        &ramdisk_ops,
        RAM_MAJOR,
        atomic_fetch_add(&next_minor, 1, ATOMIC_RELAXED),
        RAM_LBA_SIZE,
        length / RAM_LBA_SIZE
    );
    ram->base = base;
    *out = &ram->blk;
    return 0;
}
