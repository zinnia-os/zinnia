// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "buf: " fmt

#include <dev/block/buf.h>
#include <lib/initgraph.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/vmo.h>
#include <string.h>

constexpr size_t BUF_HASH_SIZE = 256;

typedef tailq_head(struct buf) buf_list_t;

static mutex_t cache_lock;
static buf_list_t hash_table[BUF_HASH_SIZE];
static buf_list_t dirty_list;

static buf_list_t* bucket(struct blockdev* dev, uint64_t blkno) {
    const uint64_t key = ((uintptr_t)dev >> 6) ^ (blkno * 0x9E3779B97F4A7C15ull);
    return &hash_table[(key ^ (key >> 32)) % BUF_HASH_SIZE];
}

static errno_t find(struct blockdev* dev, uint64_t blkno, size_t size, struct buf** out) {
    for (struct buf* it = tailq_first(bucket(dev, blkno)); it; it = tailq_next(it, hash_link)) {
        if (it->dev == dev && it->blkno == blkno) {
            if (it->size != size)
                return EINVAL;
            it->holds++;
            *out = it;
            return 0;
        }
    }
    *out = nullptr;
    return 0;
}

static void unhold(struct buf* buf) {
    // Drop the hold and unhash the buffer if it was the last one.
    mutex_lock(&cache_lock);
    const bool last = --buf->holds == 0;
    if (last)
        tailq_remove(bucket(buf->dev, buf->blkno), buf, hash_link);
    mutex_unlock(&cache_lock);

    // Free it outside the lock.
    if (last) {
        vmo_put_page(buf->page);
        mem_free(buf, sizeof(*buf));
    }
}

static void queue_dirty(struct buf* buf) {
    mutex_lock(&cache_lock);
    // The dirty list holds the buffer until it's written back.
    if (!buf->on_dirty_list) {
        tailq_insert_tail(&dirty_list, buf, dirty_link);
        buf->on_dirty_list = true;
        buf->holds++;
    }
    mutex_unlock(&cache_lock);
}

static bool dequeue_dirty(struct buf* buf) {
    mutex_lock(&cache_lock);
    const bool queued = buf->on_dirty_list;
    if (queued) {
        tailq_remove(&dirty_list, buf, dirty_link);
        buf->on_dirty_list = false;
    }
    mutex_unlock(&cache_lock);
    return queued;
}

static errno_t write_back(struct buf* buf) {
    if (!buf->dirty)
        return 0;

    // Write only this block, not the rest of its page.
    struct blockdev* dev = buf->dev;
    const size_t num_lbas = buf->size / dev->lba_size;
    const struct bio_seg seg = {
        .addr = page_addr(buf->page) + buf->blkno * buf->size % page_size(),
        .len = buf->size,
    };
    size_t done = 0;
    errno_t status = blockdev_rw(dev, BIO_WRITE, buf->blkno * num_lbas, num_lbas, &seg, 1, &done);
    if (!status && done < num_lbas)
        status = EIO;
    if (!status)
        buf->dirty = false;

    return status;
}

static errno_t lookup(struct blockdev* dev, uint64_t blkno, size_t size, enum vmo_access access, struct buf** out) {
    // Blocks have to fit in a page and on the device.
    const size_t pgsz = page_size();
    if (size == 0 || pgsz % size != 0 || size % dev->lba_size != 0)
        return EINVAL;
    if (blkno >= dev->lba_count * dev->lba_size / size)
        return EINVAL;

    // Try to find an existing buffer first.
    struct buf* buf;
    mutex_lock(&cache_lock);
    errno_t status = find(dev, blkno, size, &buf);
    mutex_unlock(&cache_lock);
    if (status)
        return status;

    if (!buf) {
        // Make a new buffer backed by the device cache page.
        struct buf* fresh = mem_alloc(sizeof(*fresh), 0);
        if (!fresh)
            return ENOMEM;
        const uint64_t offset = blkno * size;
        status = vmo_get_page(&dev->cache, offset / pgsz, access, &fresh->page);
        if (status) {
            mem_free(fresh, sizeof(*fresh));
            return status;
        }
        fresh->dev = dev;
        fresh->blkno = blkno;
        fresh->size = size;
        fresh->data = (uint8_t*)HHDM_PTR(page_addr(fresh->page)) + offset % pgsz;
        mutex_init(&fresh->lock);
        fresh->holds = 1;

        // Someone else might have inserted the block in the meantime.
        mutex_lock(&cache_lock);
        status = find(dev, blkno, size, &buf);
        if (!status && !buf) {
            buf = fresh;
            tailq_insert_tail(bucket(dev, blkno), buf, hash_link);
        }
        mutex_unlock(&cache_lock);
        if (buf != fresh) {
            vmo_put_page(fresh->page);
            mem_free(fresh, sizeof(*fresh));
        }
        if (status)
            return status;
    }

    mutex_lock(&buf->lock);
    *out = buf;
    return 0;
}

errno_t buf_read(struct blockdev* dev, uint64_t blkno, size_t size, struct buf** out) {
    return lookup(dev, blkno, size, VMO_READ, out);
}

errno_t buf_get(struct blockdev* dev, uint64_t blkno, size_t size, struct buf** out) {
    struct buf* buf;
    // If the block covers the whole page, we don't need to read it.
    errno_t status = lookup(dev, blkno, size, size == page_size() ? VMO_OVERWRITE : VMO_READ, &buf);
    if (status)
        return status;

    memset(buf->data, 0, buf->size);
    buf_mark_dirty(buf);
    *out = buf;
    return 0;
}

void buf_drop(struct buf* buf) {
    mutex_unlock(&buf->lock);
    unhold(buf);
}

void buf_mark_dirty(struct buf* buf) {
    buf->dirty = true;
    queue_dirty(buf);
}

errno_t buf_write(struct buf* buf) {
    // Write the buffer, or requeue it if that fails.
    buf->dirty = true;
    errno_t status = write_back(buf);
    if (status) {
        queue_dirty(buf);
        return status;
    }

    // The dirty list can let go of the buffer now.
    if (dequeue_dirty(buf))
        unhold(buf);
    return 0;
}

void buf_invalidate(struct blockdev* dev, uint64_t blkno, size_t size) {
    struct buf* buf;
    mutex_lock(&cache_lock);
    errno_t status = find(dev, blkno, size, &buf);
    mutex_unlock(&cache_lock);
    if (status || !buf)
        return;

    // Forget the pending write and take it off the dirty list.
    mutex_lock(&buf->lock);
    buf->dirty = false;
    if (dequeue_dirty(buf))
        unhold(buf);

    buf_drop(buf);
}

errno_t buf_sync(struct blockdev* dev) {
    // Count the dirty buffers of `dev`.
    mutex_lock(&cache_lock);
    size_t num_pending = 0;
    for (struct buf* it = tailq_first(&dirty_list); it; it = tailq_next(it, dirty_link)) {
        if (it->dev == dev)
            num_pending++;
    }

    struct buf** pending = nullptr;
    if (num_pending) {
        pending = mem_alloc(num_pending * sizeof(*pending), ALLOC_NOZERO);
        if (!pending) {
            mutex_unlock(&cache_lock);
            return ENOMEM;
        }
    }

    // Take them off the dirty list and keep its holds for ourselves.
    size_t idx = 0;
    struct buf* it = tailq_first(&dirty_list);
    while (it) {
        struct buf* next = tailq_next(it, dirty_link);
        if (it->dev == dev) {
            tailq_remove(&dirty_list, it, dirty_link);
            it->on_dirty_list = false;
            pending[idx++] = it;
        }
        it = next;
    }
    mutex_unlock(&cache_lock);

    // Write them back without the cache lock.
    errno_t result = 0;
    for (size_t i = 0; i < num_pending; i++) {
        struct buf* buf = pending[i];
        mutex_lock(&buf->lock);
        errno_t status = write_back(buf);
        if (status) {
            log_warn("Unable to write back block %lu (%i)\n", buf->blkno, status);
            if (!result)
                result = status;
            queue_dirty(buf);
        }
        buf_drop(buf);
    }
    mem_free(pending, num_pending * sizeof(*pending));

    // Make sure the writes actually hit the disk.
    errno_t status = blockdev_flush(dev);
    return result ? result : status;
}

static void buf_stage_init(struct boot_info* info) {
    mutex_init(&cache_lock);
    for (size_t i = 0; i < BUF_HASH_SIZE; i++)
        tailq_init(&hash_table[i]);
    tailq_init(&dirty_list);
}

INIT_TASK(buf_stage, "dev.block.buf", buf_stage_init);
INIT_ENTAILS(buf_stage, block_stage);
