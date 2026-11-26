// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "nvme: " fmt

#include <dev/block/nvme/nvme.h>
#include <lib/clock.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/mmio.h>
#include <string.h>

constexpr size_t DOORBELL_OFFSET = 0x1000;

static void ring_tail(void* regs, size_t doorbells_offset, size_t tail) {
    mmio_reg_write(regs, ((mmio_reg32){doorbells_offset, MMIO_LE}), (uint32_t)tail);
}

static void ring_head(void* regs, size_t doorbells_offset, size_t doorbell_stride, size_t head) {
    mmio_reg_write(regs, ((mmio_reg32){doorbells_offset + doorbell_stride, MMIO_LE}), (uint32_t)head);
}

// Creates a new submission and completion queue pair.
errno_t nvme_queue_new(
    struct dma_tag* tag,
    void* regs,
    size_t doorbell_stride,
    size_t queue_id,
    size_t depth,
    struct nvme_queue** out
) {
    const size_t align = 0x1000;
    const size_t sq_size = ROUND_UP(depth * sizeof(struct nvme_sqe), align);
    const size_t cq_size = ROUND_UP(depth * sizeof(struct nvme_cqe), align);

    struct nvme_queue* queue = mem_alloc(sizeof(*queue), 0);
    if (!queue)
        return ENOMEM;

    // Allocate memory the completion queue.
    errno_t status = dma_buffer_alloc(tag, cq_size, &queue->cq);
    if (status)
        goto fail;

    // Allocate memory for the submission queue.
    status = dma_buffer_alloc(tag, sq_size, &queue->sq);
    if (status)
        goto fail;

    // Calculate the offset of the doorbell registers. The stride is already precomputed here.
    queue->doorbells_offset = DOORBELL_OFFSET + (queue_id * 2 * doorbell_stride);
    queue->doorbell_stride = doorbell_stride;
    queue->depth = depth;
    queue->regs = regs;
    queue->cq_head = 0;
    queue->cq_phase = 1; // When the controller is enabled, the first phase is 1.
    queue->sq_tail = 0;

    *out = queue;
    return 0;

fail:
    nvme_queue_free(queue);
    return status;
}

void nvme_queue_free(struct nvme_queue* queue) {
    dma_buffer_free(&queue->cq);
    dma_buffer_free(&queue->sq);
    mem_free(queue, sizeof(*queue));
}

// Submits a command to this queue.
errno_t nvme_queue_submit_cmd(struct nvme_queue* queue, const struct nvme_sqe* sqe) {
    struct nvme_sqe* entries = queue->sq.virt;
    memcpy(&entries[queue->sq_tail], sqe, sizeof(*sqe));
    dma_buffer_sync(&queue->sq, queue->sq_tail * sizeof(*sqe), sizeof(*sqe), DMA_SYNC_PREWRITE);

    queue->sq_tail++;
    if (queue->sq_tail == queue->depth)
        queue->sq_tail = 0;

    // Notify the controller of the new tail index.
    ring_tail(queue->regs, queue->doorbells_offset, queue->sq_tail);

    return 0;
}

// Reads the next completion entry from the queue.
errno_t nvme_queue_next_completion(struct nvme_queue* queue, struct nvme_completion* out) {
    struct nvme_cqe* entry = &((struct nvme_cqe*)queue->cq.virt)[queue->cq_head];

    // Wait until the phase for this entry has changed.
    uint32_t dw3;
    const uint64_t deadline = clock_get_elapsed_ns() + NVME_COMPLETION_TIMEOUT_NS;
    while (true) {
        dma_buffer_sync(&queue->cq, queue->cq_head * sizeof(*entry), sizeof(*entry), DMA_SYNC_POSTREAD);
        dw3 = mmio_reg_read(entry, NVME_CQE_DW3);

        // The controller will flip the phase bit of the current entry when writing.
        if (mmio_field_get(NVME_CQE_PHASE_TAG, dw3) == queue->cq_phase)
            break;

        if (clock_get_elapsed_ns() >= deadline)
            return ETIMEDOUT;
    }

    // Then, read the rest of the completion queue entry.
    uint32_t dw0 = mmio_reg_read(entry, NVME_CQE_DW0);
    uint32_t dw2 = mmio_reg_read(entry, NVME_CQE_DW2);

    *out = (struct nvme_completion){
        .result = dw0,
        .sq_head = mmio_field_get(NVME_CQE_SQ_HEAD, dw2),
        .sq_id = mmio_field_get(NVME_CQE_SQ_IDENT, dw2),
        .cmd_id = mmio_field_get(NVME_CQE_CID, dw3),
        .status = mmio_field_get(NVME_CQE_STATUS, dw3),
        .phase_tag = mmio_field_get(NVME_CQE_PHASE_TAG, dw3) != 0,
    };

    queue->cq_head++;
    if (queue->cq_head == queue->depth) {
        queue->cq_head = 0;
        queue->cq_phase ^= 1; // Flip the phase bit.
    }

    // Notify the controller of the new head index.
    ring_head(queue->regs, queue->doorbells_offset, queue->doorbell_stride, queue->cq_head);

    return 0;
}

errno_t nvme_io_queue_new(
    struct dma_tag* tag,
    void* regs,
    size_t doorbell_stride,
    size_t queue_id,
    size_t depth,
    bool poll,
    struct nvme_io_queue** out
) {
    const size_t align = 0x1000;
    const size_t sq_size = ROUND_UP(depth * sizeof(struct nvme_sqe), align);
    const size_t cq_size = ROUND_UP(depth * sizeof(struct nvme_cqe), align);

    struct nvme_io_queue* queue = mem_alloc(sizeof(*queue), 0);
    if (!queue)
        return ENOMEM;
    queue->depth = depth;

    errno_t status = dma_buffer_alloc(tag, cq_size, &queue->cq);
    if (status)
        goto fail;

    status = dma_buffer_alloc(tag, sq_size, &queue->sq);
    if (status)
        goto fail;

    const size_t slots = depth - 1;
    queue->slots = mem_alloc(slots * sizeof(*queue->slots), 0);
    queue->free = mem_alloc(slots * sizeof(*queue->free), 0);
    if (!queue->slots || !queue->free) {
        status = ENOMEM;
        goto fail;
    }
    for (size_t i = 0; i < slots; i++)
        queue->free[i] = slots - 1 - i;
    queue->num_free = slots;

    queue->queue_id = queue_id;
    queue->doorbells_offset = DOORBELL_OFFSET + (queue_id * 2 * doorbell_stride);
    queue->doorbell_stride = doorbell_stride;
    queue->regs = regs;
    queue->poll = poll;
    queue->tag = tag;
    queue->sq_tail = 0;
    queue->cq_head = 0;
    queue->cq_phase = 1;
    spin_init(&queue->sq_lock);
    spin_init(&queue->cq_lock);
    spin_init(&queue->cmds_lock);
    event_init(&queue->slots_free);

    *out = queue;
    return 0;

fail:
    nvme_io_queue_free(queue);
    return status;
}

void nvme_io_queue_free(struct nvme_io_queue* queue) {
    dma_buffer_free(&queue->cq);
    dma_buffer_free(&queue->sq);
    mem_free(queue->slots, (queue->depth - 1) * sizeof(*queue->slots));
    mem_free(queue->free, (queue->depth - 1) * sizeof(*queue->free));
    mem_free(queue, sizeof(*queue));
}

static uint16_t acquire_slot(struct nvme_io_queue* queue) {
    while (true) {
        spin_lock(&queue->cmds_lock);
        if (queue->num_free) {
            uint16_t cid = queue->free[--queue->num_free];
            spin_unlock(&queue->cmds_lock);
            return cid;
        }
        spin_unlock(&queue->cmds_lock);

        if (queue->poll) {
            nvme_io_queue_drain(queue);
            continue;
        }

        nvme_io_queue_drain(queue);
        event_reset(&queue->slots_free);
        spin_lock(&queue->cmds_lock);
        bool empty = queue->num_free == 0;
        spin_unlock(&queue->cmds_lock);
        if (empty)
            event_wait(&queue->slots_free);
    }
}

static void release_slot(struct nvme_io_queue* queue, uint16_t cid) {
    spin_lock(&queue->cmds_lock);
    queue->free[queue->num_free++] = cid;
    spin_unlock(&queue->cmds_lock);
}

static void write_sqe(struct nvme_io_queue* queue, const struct nvme_sqe* sqe) {
    spin_lock(&queue->sq_lock);
    struct nvme_sqe* entries = queue->sq.virt;
    memcpy(&entries[queue->sq_tail], sqe, sizeof(*sqe));
    dma_buffer_sync(&queue->sq, queue->sq_tail * sizeof(*sqe), sizeof(*sqe), DMA_SYNC_PREWRITE);
    queue->sq_tail++;
    if (queue->sq_tail == queue->depth)
        queue->sq_tail = 0;
    ring_tail(queue->regs, queue->doorbells_offset, queue->sq_tail);
    spin_unlock(&queue->sq_lock);
}

uint16_t nvme_io_queue_submit(
    struct nvme_io_queue* queue,
    struct bio* bio,
    struct nvme_rw_cmd cmd,
    _Atomic(bool)* done
) {
    const uint16_t cid = acquire_slot(queue);
    struct nvme_inflight* slot = &queue->slots[cid];

    struct nvme_sqe sqe = {};
    if (bio->op == BIO_FLUSH) {
        slot->map = nullptr;
        slot->prps = (struct dma_buffer){};
        nvme_flush_cmd_write(&(struct nvme_flush_cmd){.cid = cid, .nsid = cmd.nsid}, &sqe);
    } else {
        errno_t status = nvme_build_prps(queue->tag, bio, slot, &cmd.prp1, &cmd.prp2);
        if (status) {
            release_slot(queue, cid);
            bio_complete(bio, EIO, 0);
            if (done)
                atomic_store(done, true, ATOMIC_RELEASE);
            return cid;
        }
        cmd.cid = cid;
        nvme_rw_cmd_write(&cmd, &sqe);
    }

    spin_lock(&queue->cmds_lock);
    slot->active = true;
    slot->bio = bio;
    slot->lbas = bio->num_lbas;
    slot->done = done;
    spin_unlock(&queue->cmds_lock);

    write_sqe(queue, &sqe);
    return cid;
}

bool nvme_io_queue_abandon(struct nvme_io_queue* queue, uint16_t cid, struct bio* bio) {
    spin_lock(&queue->cmds_lock);
    struct nvme_inflight* slot = &queue->slots[cid];
    bool abandoned = slot->active && slot->bio == bio;
    if (abandoned) {
        slot->bio = nullptr;
        slot->done = nullptr;
    }
    spin_unlock(&queue->cmds_lock);
    return abandoned;
}

size_t nvme_io_queue_drain(struct nvme_io_queue* queue) {
    size_t completed = 0;

    while (true) {
        spin_lock(&queue->cq_lock);
        struct nvme_cqe* entry = &((struct nvme_cqe*)queue->cq.virt)[queue->cq_head];
        dma_buffer_sync(&queue->cq, queue->cq_head * sizeof(*entry), sizeof(*entry), DMA_SYNC_POSTREAD);
        uint32_t dw3 = mmio_reg_read(entry, NVME_CQE_DW3);
        if (mmio_field_get(NVME_CQE_PHASE_TAG, dw3) != queue->cq_phase) {
            if (completed > 0)
                ring_head(queue->regs, queue->doorbells_offset, queue->doorbell_stride, queue->cq_head);
            spin_unlock(&queue->cq_lock);
            break;
        }

        uint16_t cid = mmio_field_get(NVME_CQE_CID, dw3);
        uint16_t status = mmio_field_get(NVME_CQE_STATUS, dw3);

        queue->cq_head++;
        if (queue->cq_head == queue->depth) {
            queue->cq_head = 0;
            queue->cq_phase ^= 1;
        }

        struct nvme_inflight inflight = {};
        spin_lock(&queue->cmds_lock);
        if (cid < queue->depth - 1 && queue->slots[cid].active) {
            inflight = queue->slots[cid];
            queue->slots[cid].active = false;
            queue->free[queue->num_free++] = cid;
        }
        spin_unlock(&queue->cmds_lock);
        spin_unlock(&queue->cq_lock);

        if (inflight.active) {
            nvme_release_prps(&inflight);
            if (inflight.bio) {
                if (nvme_status_is_success(status))
                    bio_complete(inflight.bio, 0, inflight.lbas);
                else
                    bio_complete(inflight.bio, EIO, 0);
            }
            if (inflight.done)
                atomic_store(inflight.done, true, ATOMIC_RELEASE);
        }

        completed++;
    }

    if (completed > 0)
        event_set(&queue->slots_free);

    return completed;
}
