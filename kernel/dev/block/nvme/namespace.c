// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "nvme: " fmt

#include <dev/block/nvme/nvme.h>
#include <lib/clock.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <stdckdint.h>

static errno_t nvme_ns_submit(struct blockdev* blk, struct bio* bio) {
    struct nvme_ns* ns = containerof(blk, struct nvme_ns, blk);

    if (bio->op != BIO_FLUSH) {
        uint64_t end_lba;
        if (ckd_add(&end_lba, bio->lba, (uint64_t)bio->num_lbas)) {
            bio_complete(bio, EOVERFLOW, 0);
            return 0;
        }
        if (end_lba > ns->lba_count) {
            bio_complete(bio, bio->op == BIO_READ ? 0 : ENOSPC, 0);
            return 0;
        }
    }

    struct nvme_io_queue* queue = nvme_ctrl_io_queue(ns->ctrl);
    if (!queue) {
        bio_complete(bio, EIO, 0);
        return 0;
    }

    size_t bytes = 0;
    for (size_t i = 0; i < bio->num_segs; i++)
        bytes += bio->segs[i].len;

    _Atomic(bool) done = false;
    const bool poll = queue->poll;

    uint16_t cid = nvme_io_queue_submit(
        queue,
        bio,
        (struct nvme_rw_cmd){
            .prp1 = 0,
            .prp2 = 0,
            .cid = 0,
            .do_write = bio->op == BIO_WRITE,
            .start_lba = bio->lba,
            .num_lbas = bio->num_lbas,
            .bytes = bytes,
            .control = 0,
            .ds_mgmt = 0,
            .ref_tag = 0,
            .app_tag = 0,
            .app_mask = 0,
            .nsid = ns->nsid,
        },
        poll ? &done : nullptr
    );

    // If we're not interrupt-driven, poll the IO completion queue.
    if (poll) {
        const uint64_t deadline = clock_get_elapsed_ns() + NVME_COMPLETION_TIMEOUT_NS;
        while (!atomic_load(&done, ATOMIC_ACQUIRE)) {
            nvme_io_queue_drain(queue);
            if (atomic_load(&done, ATOMIC_ACQUIRE))
                break;
            if (clock_get_elapsed_ns() >= deadline && nvme_io_queue_abandon(queue, cid, bio)) {
                bio_complete(bio, EIO, 0);
                break;
            }
        }
    }

    return 0;
}

static void nvme_ns_destroy(struct blockdev* blk) {
    mem_free(containerof(blk, struct nvme_ns, blk), sizeof(struct nvme_ns));
}

static const struct blockdev_ops nvme_ns_ops = {
    .submit = nvme_ns_submit,
    .destroy = nvme_ns_destroy,
};

errno_t nvme_ns_new(
    struct nvme_ctrl* ctrl,
    uint32_t nsid,
    uint8_t lba_shift,
    uint64_t lba_count,
    size_t max_transfer_bytes,
    struct nvme_ns** out
) {
    log("Namespace %u: LBA size %zu bytes, %zu MBs total\n",
        nsid,
        (size_t)1 << lba_shift,
        (size_t)(lba_count << lba_shift) / 1024 / 1024);

    struct nvme_ns* ns = mem_alloc(sizeof(*ns), 0);
    if (!ns)
        return ENOMEM;

    blockdev_init(&ns->blk, &nvme_ns_ops, NVME_MAJOR, nsid, (size_t)1 << lba_shift, lba_count);
    ns->blk.limits = (struct blockdev_limits){
        .max_lbas = MAX(max_transfer_bytes >> lba_shift, 1),
        .max_segs = 512,
    };
    ns->ctrl = ctrl;
    ns->nsid = nsid;
    ns->lba_shift = lba_shift;
    ns->lba_count = lba_count;
    ns->max_transfer_bytes = max_transfer_bytes;

    *out = ns;
    return 0;
}
