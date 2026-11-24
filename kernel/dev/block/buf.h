// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <dev/block/blockdev.h>
#include <lib/errno.h>
#include <lib/mutex.h>
#include <lib/tailq.h>
#include <stddef.h>
#include <stdint.h>

struct buf {
    struct blockdev* dev;
    uint64_t blkno;
    size_t size;
    struct page* page;
    void* data;

    mutex_t lock;
    bool dirty;

    size_t holds;
    bool on_dirty_list;
    tailq_link(struct buf) hash_link;
    tailq_link(struct buf) dirty_link;
};

errno_t buf_read(struct blockdev* dev, uint64_t blkno, size_t size, struct buf** out);

errno_t buf_get(struct blockdev* dev, uint64_t blkno, size_t size, struct buf** out);

void buf_drop(struct buf* buf);

void buf_mark_dirty(struct buf* buf);

errno_t buf_write(struct buf* buf);

void buf_invalidate(struct blockdev* dev, uint64_t blkno, size_t size);

errno_t buf_sync(struct blockdev* dev);
