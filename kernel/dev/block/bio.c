// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>

struct bio* bio_alloc(enum bio_op op, size_t num_segs) {
    struct bio* bio = mem_alloc(sizeof(struct bio) + num_segs * sizeof(struct bio_seg), 0);
    if (!bio)
        return nullptr;
    bio->op = op;
    bio->num_segs = num_segs;
    bio->max_segs = num_segs;
    event_init(&bio->event);
    atomic_init(&bio->pending, 0);
    atomic_init(&bio->short_offset, UINT64_MAX);
    atomic_init(&bio->child_status, 0);
    return bio;
}

void bio_free(struct bio* bio) {
    mem_free(bio, sizeof(*bio) + bio->max_segs * sizeof(struct bio_seg));
}

static void record_short(struct bio* parent, uint64_t offset, errno_t status) {
    uint64_t current = atomic_load(&parent->short_offset, ATOMIC_RELAXED);
    while (offset < current) {
        if (atomic_compare_exchange_weak(&parent->short_offset, &current, offset, ATOMIC_RELAXED, ATOMIC_RELAXED))
            break;
    }
    if (status) {
        errno_t expected = 0;
        atomic_compare_exchange(&parent->child_status, &expected, status, ATOMIC_RELAXED, ATOMIC_RELAXED);
    }
}

static void parent_put(struct bio* parent) {
    if (atomic_fetch_sub(&parent->pending, 1, ATOMIC_ACQ_REL) != 1)
        return;
    const uint64_t offset = atomic_load(&parent->short_offset, ATOMIC_RELAXED);
    const size_t done = offset == UINT64_MAX ? parent->num_lbas : offset;
    const errno_t status = atomic_load(&parent->child_status, ATOMIC_RELAXED);
    bio_complete(parent, done == 0 ? status : 0, done);
}

void bio_complete(struct bio* bio, errno_t status, size_t done_lbas) {
    bio->status = status;
    bio->done_lbas = done_lbas;

    struct bio* parent = bio->parent;
    if (parent) {
        if (status || done_lbas < bio->num_lbas)
            record_short(parent, bio->parent_offset + done_lbas, status);
        bio_free(bio);
        parent_put(parent);
        return;
    }

    if (bio->end)
        bio->end(bio);
    else
        event_set(&bio->event);
}

errno_t bio_wait(struct bio* bio, size_t* out_lbas) {
    event_wait(&bio->event);
    if (out_lbas)
        *out_lbas = bio->done_lbas;
    return bio->status;
}

// Submits one bio and completes early if it failed.
static void submit_one(struct blockdev* dev, struct bio* bio) {
    errno_t status = dev->ops->submit(dev, bio);
    if (status)
        bio_complete(bio, status, 0);
}

void bio_submit(struct blockdev* dev, struct bio* bio) {
    bio->dev = dev;
    if (bio->op == BIO_FLUSH) {
        submit_one(dev, bio);
        return;
    }

    if (bio->op == BIO_READ) {
        if (bio->lba >= dev->lba_count) {
            bio_complete(bio, 0, 0);
            return;
        }
        bio->num_lbas = MIN(bio->num_lbas, dev->lba_count - bio->lba);
    }
    if (bio->num_lbas == 0) {
        bio_complete(bio, 0, 0);
        return;
    }

    const size_t lba_size = dev->lba_size;
    const size_t lbas_per_page = MAX(page_size() / lba_size, 1);
    size_t max_lbas = dev->limits.max_lbas ? dev->limits.max_lbas : SIZE_MAX;
    if (max_lbas > lbas_per_page)
        max_lbas -= max_lbas % lbas_per_page;
    const size_t max_segs = dev->limits.max_segs ? dev->limits.max_segs : SIZE_MAX;

    if (bio->num_lbas <= max_lbas && bio->num_segs <= max_segs) {
        submit_one(dev, bio);
        return;
    }

    atomic_store(&bio->pending, 1, ATOMIC_RELAXED);
    atomic_store(&bio->short_offset, UINT64_MAX, ATOMIC_RELAXED);
    atomic_store(&bio->child_status, 0, ATOMIC_RELAXED);

    size_t seg = 0;
    size_t seg_off = 0;
    uint64_t consumed = 0;
    while (consumed < bio->num_lbas) {
        const size_t cap = MIN(max_segs, bio->num_segs - seg);
        struct bio* child = cap ? bio_alloc(bio->op, cap) : nullptr;
        if (!child) {
            record_short(bio, consumed, cap ? ENOMEM : EINVAL);
            break;
        }

        size_t count = 0;
        size_t lbas = 0;
        while (lbas < max_lbas && count < cap && consumed + lbas < bio->num_lbas) {
            const struct bio_seg* src = &bio->segs[seg];
            const size_t take =
                MIN(MIN(src->len - seg_off, (max_lbas - lbas) * lba_size),
                    (bio->num_lbas - consumed - lbas) * lba_size);
            if (take < lba_size)
                break;
            child->segs[count++] = (struct bio_seg){.addr = src->addr + seg_off, .len = take};
            lbas += take / lba_size;
            seg_off += take;
            if (seg_off == src->len) {
                seg++;
                seg_off = 0;
            }
        }

        if (lbas == 0) {
            bio_free(child);
            record_short(bio, consumed, EINVAL);
            break;
        }

        child->num_segs = count;
        child->num_lbas = lbas;
        child->lba = bio->lba + consumed;
        child->parent = bio;
        child->parent_offset = consumed;
        child->dev = dev;
        consumed += lbas;
        atomic_fetch_add(&bio->pending, 1, ATOMIC_RELAXED);
        submit_one(dev, child);
    }

    parent_put(bio);
}
