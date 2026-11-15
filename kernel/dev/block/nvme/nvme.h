// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <dev/block/blockdev.h>
#include <dev/block/nvme/spec.h>
#include <dev/pci/pci.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/event.h>
#include <lib/mutex.h>
#include <lib/spin.h>
#include <lib/vec.h>
#include <mem/dma.h>
#include <stddef.h>
#include <stdint.h>

constexpr uint32_t NVME_MAJOR = 159;
constexpr uint64_t NVME_COMPLETION_TIMEOUT_NS = 30'000'000'000;

// An entry in the completion queue.
struct nvme_completion {
    uint32_t result;
    uint16_t sq_head;
    uint16_t sq_id;
    uint16_t cmd_id;
    bool phase_tag;
    uint16_t status;
};

static inline bool nvme_status_is_success(uint16_t status) {
    return status == 0;
}

struct nvme_rw_cmd {
    uint64_t prp1;
    uint64_t prp2;
    uint16_t cid;
    bool do_write;
    uint64_t start_lba;
    size_t num_lbas;
    size_t bytes;
    uint16_t control;
    uint32_t ds_mgmt;
    uint32_t ref_tag;
    uint16_t app_tag;
    uint16_t app_mask;
    uint32_t nsid;
};

struct nvme_flush_cmd {
    uint16_t cid;
    uint32_t nsid;
};

struct nvme_create_cq_cmd {
    dma_addr_t cq_addr;
    size_t depth;
    size_t qid;
    bool irqs_enabled;
    uint16_t irq_vector;
};

struct nvme_create_sq_cmd {
    dma_addr_t sq_addr;
    size_t depth;
    size_t qid;
    size_t cqid;
};

struct nvme_identify_cmd {
    dma_addr_t buffer;
    uint16_t controller_id;
    uint8_t cns;
    uint32_t nsid;
};

void nvme_rw_cmd_write(const struct nvme_rw_cmd* cmd, struct nvme_sqe* sqe);
void nvme_flush_cmd_write(const struct nvme_flush_cmd* cmd, struct nvme_sqe* sqe);
void nvme_create_cq_cmd_write(const struct nvme_create_cq_cmd* cmd, struct nvme_sqe* sqe);
void nvme_create_sq_cmd_write(const struct nvme_create_sq_cmd* cmd, struct nvme_sqe* sqe);
void nvme_identify_cmd_write(const struct nvme_identify_cmd* cmd, struct nvme_sqe* sqe);

struct nvme_queue {
    // Amount of queue entries.
    size_t depth;
    size_t doorbells_offset;
    size_t doorbell_stride;
    void* regs;
    // Physical buffer for the completion queue.
    struct dma_buffer cq;
    // The index of the current completion queue entry.
    size_t cq_head;
    // Determines whether a completion queue entry is new.
    uint8_t cq_phase;
    // Physical buffer for the submission queue.
    struct dma_buffer sq;
    // The index of the current submission queue entry.
    size_t sq_tail;
};

errno_t nvme_queue_new(
    struct dma_tag* tag,
    void* regs,
    size_t doorbell_stride,
    size_t queue_id,
    size_t depth,
    struct nvme_queue** out
);
void nvme_queue_free(struct nvme_queue* queue);
errno_t nvme_queue_submit_cmd(struct nvme_queue* queue, const struct nvme_sqe* sqe);
errno_t nvme_queue_next_completion(struct nvme_queue* queue, struct nvme_completion* out);

struct nvme_inflight {
    bool active;
    struct bio* bio;
    size_t lbas;
    struct dma_map* map;
    struct dma_buffer prps;
    _Atomic(bool)* done;
};

struct nvme_io_queue {
    size_t queue_id;
    size_t depth;
    size_t doorbells_offset;
    size_t doorbell_stride;
    void* regs;
    bool poll;
    struct dma_tag* tag;
    struct dma_buffer cq;
    struct dma_buffer sq;
    struct spinlock sq_lock;
    size_t sq_tail;
    struct spinlock cq_lock;
    size_t cq_head;
    uint8_t cq_phase;
    struct spinlock cmds_lock;
    struct nvme_inflight* slots;
    uint16_t* free;
    size_t num_free;
    struct event slots_free;
};

errno_t nvme_io_queue_new(
    struct dma_tag* tag,
    void* regs,
    size_t doorbell_stride,
    size_t queue_id,
    size_t depth,
    bool poll,
    struct nvme_io_queue** out
);
void nvme_io_queue_free(struct nvme_io_queue* queue);
uint16_t nvme_io_queue_submit(
    struct nvme_io_queue* queue,
    struct bio* bio,
    struct nvme_rw_cmd cmd,
    _Atomic(bool)* done
);
bool nvme_io_queue_abandon(struct nvme_io_queue* queue, uint16_t cid, struct bio* bio);
size_t nvme_io_queue_drain(struct nvme_io_queue* queue);

errno_t nvme_build_prps(
    struct dma_tag* tag,
    struct bio* bio,
    struct nvme_inflight* slot,
    uint64_t* prp1,
    uint64_t* prp2
);
void nvme_release_prps(struct nvme_inflight* slot);

struct nvme_ns;

struct nvme_ctrl {
    void* regs;
    struct dma_tag* dma_tag;
    size_t doorbell_stride; // The amount of bytes between two doorbells.
    size_t mps_min;
    uint8_t mps;

    char serial[20 + 1];  // Serial number of the controller.
    char model[40 + 1];   // Model name of the controller.
    char revision[8 + 1]; // Revision of the controller's firmware.

    struct nvme_queue* admin_queue; // Queue to submit admin commands to.
    mutex_t admin_lock;

    struct nvme_io_queue* io_queue; // Queue to submit IO commands to.
    struct spinlock io_lock;

    _Atomic(uint32_t) ns_count; // Amount of namespaces.
    size_t mdts;                // Maximum data transfer size.
};

typedef vec(struct nvme_ns*) nvme_ns_vec_t;

errno_t nvme_ctrl_new_pci(void* regs, struct dma_tag* dma_tag, struct nvme_ctrl** out);
size_t nvme_ctrl_max_transfer_bytes(struct nvme_ctrl* ctrl);
errno_t nvme_ctrl_reset(struct nvme_ctrl* ctrl, bool irqs_enabled, uint16_t io_irq_vector);
errno_t nvme_ctrl_identify(struct nvme_ctrl* ctrl);
errno_t nvme_ctrl_scan_namespaces(struct nvme_ctrl* ctrl, nvme_ns_vec_t* out);
struct nvme_io_queue* nvme_ctrl_io_queue(struct nvme_ctrl* ctrl);

struct nvme_ns {
    struct blockdev blk;
    struct nvme_ctrl* ctrl;
    uint32_t nsid;
    uint8_t lba_shift;
    uint64_t lba_count;
    size_t max_transfer_bytes;
};

errno_t nvme_ns_new(
    struct nvme_ctrl* ctrl,
    uint32_t nsid,
    uint8_t lba_shift,
    uint64_t lba_count,
    size_t max_transfer_bytes,
    struct nvme_ns** out
);
