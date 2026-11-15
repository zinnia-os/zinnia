// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/vfs.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/event.h>
#include <lib/guid.h>
#include <lib/initgraph.h>
#include <lib/types.h>
#include <mem/vmo.h>
#include <stddef.h>
#include <stdint.h>

enum bio_op : uint8_t {
    BIO_READ,
    BIO_WRITE,
    BIO_FLUSH,
};

struct bio_seg {
    phys_t addr;
    size_t len;
};

struct blockdev;

// A block IO operation.
struct bio {
    enum bio_op op;               // What to do.
    uint64_t lba;                 // Starting block.
    size_t num_lbas;              // Amount of blocks to read.
    struct blockdev* dev;         // Device to use for this bio.
    errno_t status;               // Result of this operation.
    size_t done_lbas;             // Current progress.
    void (*end)(struct bio* bio); // Callback which runs when the bio completes.
    void* priv;                   // Driver data.
    struct bio* parent;           // For nested bios.
    uint64_t parent_offset;
    _Atomic(size_t) pending;
    _Atomic(uint64_t) short_offset;
    _Atomic(errno_t) child_status;
    struct event event; // Event for poll.
    size_t max_segs;
    size_t num_segs; // Counter for `segs`.
    struct bio_seg segs[];
};

struct blockdev_limits {
    size_t max_lbas;
    size_t max_segs;
};

struct blockdev_ops {
    // Submit a bio for execution on this block device.
    errno_t (*submit)(struct blockdev* dev, struct bio* bio);
    // Handles an ioctl for the block device.
    errno_t (*ioctl)(struct blockdev* dev, struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out);
    void (*destroy)(struct blockdev* dev);
};

struct blockdev {
    struct vmo cache;
    const struct blockdev_ops* ops;
    dev_t dev;
    size_t lba_size;
    uint64_t lba_count;
    struct blockdev_limits limits;
    _Atomic(bool) claimed;
};

struct bio* bio_alloc(enum bio_op op, size_t num_segs);

void bio_free(struct bio* bio);

// Submit a bio to run on `dev`.
void bio_submit(struct blockdev* dev, struct bio* bio);

void bio_complete(struct bio* bio, errno_t status, size_t done_lbas);

errno_t bio_wait(struct bio* bio, size_t* out_lbas);

void blockdev_init(
    struct blockdev* dev,
    const struct blockdev_ops* ops,
    uint32_t major,
    uint32_t minor,
    size_t lba_size,
    uint64_t lba_count
);
void blockdev_ref(struct blockdev* dev);
void blockdev_unref(struct blockdev* dev);

// Performs IO on a block device using `bio_seg` segments.
errno_t blockdev_rw(
    struct blockdev* dev,
    enum bio_op op,
    uint64_t lba,
    size_t num_lbas,
    const struct bio_seg* segs,
    size_t num_segs,
    size_t* out_lbas
);

// Performs IO on a block device using pages.
errno_t blockdev_rw_pages(
    struct blockdev* dev,
    enum bio_op op,
    uint64_t lba,
    size_t num_lbas,
    struct page** pages,
    size_t count,
    size_t* out_lbas
);

// Flushes out all outstanding bios on `dev`.
errno_t blockdev_flush(struct blockdev* dev);

errno_t blockdev_from_node(struct vfs_node* node, struct blockdev** out);
errno_t blockdev_claim(struct blockdev* dev);
void blockdev_release(struct blockdev* dev);

// Adds a new block device to the list of known devices.
// Will also add a node in `/dev/block`.
errno_t blockdev_register(const char* name, struct blockdev* dev);

// Rescans `dev` for partitions or file systems.
errno_t blockdev_rescan(struct blockdev* dev);

// Syncs all outstanding requests blockdevs
errno_t blockdev_sync_all();

errno_t devtmpfs_add_blockdev(const char* name, struct blockdev* dev, mode_t mode);

errno_t partition_new(struct blockdev* parent, uint64_t start_lba, uint64_t lba_count, struct blockdev** out);

struct gpt_partition {
    struct guid type_guid;
    struct guid unique_guid;
    uint64_t start_lba;
    uint64_t end_lba;
};

errno_t gpt_scan(struct blockdev* dev, struct gpt_partition** out, size_t* out_count, size_t* out_capacity);

errno_t ramdisk_new(phys_t base, size_t length, struct blockdev** out);

INIT_EXTERN(block_stage);
