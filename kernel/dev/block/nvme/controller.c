// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "nvme: " fmt

#include <dev/block/nvme/nvme.h>
#include <lib/clock.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/mmio.h>
#include <mem/page.h>
#include <string.h>

constexpr size_t ADMIN_QUEUE_SIZE = 32;
constexpr size_t IO_QUEUE_SIZE_MAX = 1024;
constexpr size_t IDENTIFY_SIZE = 4096;
constexpr size_t NS_LIST_LEN = 1024;

constexpr mmio_reg8 ID_MDTS = {77, MMIO_LE};
constexpr mmio_reg32 ID_NN = {516, MMIO_LE};
constexpr mmio_reg64 ID_NSZE = {0, MMIO_LE};
constexpr mmio_reg8 ID_FLBAS = {26, MMIO_LE};
constexpr mmio_field8 ID_LBAF_IDX = {ID_FLBAS, 0, 4};
constexpr mmio_reg32 ID_LBAF0 = {128, MMIO_LE};
constexpr mmio_field32 ID_LBADS = {ID_LBAF0, 16, 8};

size_t nvme_ctrl_max_transfer_bytes(struct nvme_ctrl* ctrl) {
    const size_t cap = 512 * page_size();
    if (ctrl->mdts != 0)
        return MIN(ctrl->mdts, cap);
    return cap;
}

errno_t nvme_ctrl_new_pci(void* regs, struct dma_tag* dma_tag, struct nvme_ctrl** out) {
    // Read controller version.
    uint32_t vs = mmio_reg_read(regs, NVME_REG_VS);
    log("NVMe controller version %u.%u.%u\n",
        mmio_field_get(NVME_VS_MJR, vs),
        mmio_field_get(NVME_VS_MNR, vs),
        mmio_field_get(NVME_VS_TER, vs));

    uint64_t cap = mmio_reg_read(regs, NVME_REG_CAP);

    // Check if our host page size is supported.
    const size_t mps_max = (size_t)1 << (mmio_field_get(NVME_CAP_MPSMAX, cap) + 12);
    const size_t mps_min = (size_t)1 << (mmio_field_get(NVME_CAP_MPSMIN, cap) + 12);
    const size_t pgsz = page_size();
    if (pgsz < mps_min || pgsz > mps_max) {
        log_err("Host page size is not supported on this NVMe!\n");
        return ENOTSUP;
    }

    struct nvme_ctrl* ctrl = mem_alloc(sizeof(*ctrl), 0);
    if (!ctrl)
        return ENOMEM;

    ctrl->regs = regs;
    ctrl->dma_tag = dma_tag;
    ctrl->doorbell_stride = (size_t)4 << mmio_field_get(NVME_CAP_DSTRD, cap);
    ctrl->mps_min = mps_min;
    ctrl->mps = __builtin_ctzl(pgsz) - 12;
    mutex_init(&ctrl->admin_lock);
    spin_init(&ctrl->io_lock);
    atomic_init(&ctrl->ns_count, 0);

    *out = ctrl;
    return 0;
}

// Sets the CC.EN flag.
static errno_t set_status(struct nvme_ctrl* ctrl, bool enable) {
    uint64_t cap = mmio_reg_read(ctrl->regs, NVME_REG_CAP);
    // CAP.TO is in 500ms units.
    const uint64_t timeout = MAX((uint64_t)mmio_field_get(NVME_CAP_TO, cap), 1) * 500'000'000;
    const uint64_t deadline = clock_get_elapsed_ns() + timeout;

    uint32_t cc = mmio_reg_read(ctrl->regs, NVME_REG_CC);
    cc &= ~mmio_field(NVME_CC_EN, UINT32_MAX);
    cc |= mmio_field(NVME_CC_EN, enable ? 1 : 0);
    mmio_reg_write(ctrl->regs, NVME_REG_CC, cc);

    while (true) {
        uint32_t csts = mmio_reg_read(ctrl->regs, NVME_REG_CSTS);

        if (mmio_field_get(NVME_CSTS_CFS, csts) != 0)
            return EIO;

        bool ready = mmio_field_get(NVME_CSTS_RDY, csts) != 0;
        if (ready == enable)
            break;

        if (clock_get_elapsed_ns() >= deadline)
            return ETIMEDOUT;
    }

    return 0;
}

static errno_t admin_cmd(struct nvme_queue* queue, const struct nvme_sqe* sqe, struct nvme_completion* out) {
    errno_t status = nvme_queue_submit_cmd(queue, sqe);
    if (status)
        return status;
    return nvme_queue_next_completion(queue, out);
}

static errno_t identify(struct nvme_queue* queue, const struct nvme_identify_cmd* cmd) {
    struct nvme_sqe sqe = {};
    nvme_identify_cmd_write(cmd, &sqe);
    struct nvme_completion comp;
    errno_t status = admin_cmd(queue, &sqe, &comp);
    if (status)
        return status;
    if (!nvme_status_is_success(comp.status)) {
        log_err("Identification failed with status %x\n", comp.status);
        return EIO;
    }
    return 0;
}

errno_t nvme_ctrl_reset(struct nvme_ctrl* ctrl, bool irqs_enabled, uint16_t io_irq_vector) {
    // Disable the controller.
    errno_t status = set_status(ctrl, false);
    if (status)
        return status;

    uint64_t cap = mmio_reg_read(ctrl->regs, NVME_REG_CAP);

    const size_t queue_depth = MIN((size_t)mmio_field_get(NVME_CAP_MQES, cap) + 1, IO_QUEUE_SIZE_MAX);

    // Create an admin queue first so we can create more queues.
    struct nvme_queue* admin_queue;
    status = nvme_queue_new(ctrl->dma_tag, ctrl->regs, ctrl->doorbell_stride, 0, ADMIN_QUEUE_SIZE, &admin_queue);
    if (status)
        return status;

    // Set the admin queue sizes.
    uint32_t aqa = 0;
    aqa |= mmio_field(NVME_AQA_ACQS, admin_queue->depth - 1);
    aqa |= mmio_field(NVME_AQA_ASQS, admin_queue->depth - 1);
    mmio_reg_write(ctrl->regs, NVME_REG_AQA, aqa);

    // Set the addresses to the admin submission and completion queue.
    mmio_reg_write(ctrl->regs, NVME_REG_ASQ, admin_queue->sq.addr);
    mmio_reg_write(ctrl->regs, NVME_REG_ACQ, admin_queue->cq.addr);

    uint32_t cc = mmio_reg_read(ctrl->regs, NVME_REG_CC);
    cc &=
        ~(mmio_field(NVME_CC_MPS, UINT32_MAX) | mmio_field(NVME_CC_IOCQES, UINT32_MAX) |
          mmio_field(NVME_CC_IOSQES, UINT32_MAX));

    cc |= mmio_field(NVME_CC_MPS, ctrl->mps);
    cc |= mmio_field(NVME_CC_IOCQES, __builtin_ctzl(sizeof(struct nvme_cqe)));
    cc |= mmio_field(NVME_CC_IOSQES, __builtin_ctzl(sizeof(struct nvme_sqe)));

    mmio_reg_write(ctrl->regs, NVME_REG_CC, cc);

    // Re-enable the controller.
    status = set_status(ctrl, true);
    if (status)
        goto fail_admin;

    struct nvme_io_queue* io_queue;
    status =
        nvme_io_queue_new(ctrl->dma_tag, ctrl->regs, ctrl->doorbell_stride, 1, queue_depth, !irqs_enabled, &io_queue);
    if (status)
        goto fail_admin;

    struct nvme_sqe sqe = {};
    struct nvme_completion comp;
    nvme_create_cq_cmd_write(
        &(struct nvme_create_cq_cmd){
            .cq_addr = io_queue->cq.addr,
            .depth = io_queue->depth,
            .qid = io_queue->queue_id,
            .irqs_enabled = irqs_enabled,
            .irq_vector = irqs_enabled ? io_irq_vector : 0,
        },
        &sqe
    );
    status = admin_cmd(admin_queue, &sqe, &comp);
    if (!status && !nvme_status_is_success(comp.status))
        status = EIO;
    if (status)
        goto fail_io;

    sqe = (struct nvme_sqe){};
    nvme_create_sq_cmd_write(
        &(struct nvme_create_sq_cmd){
            .sq_addr = io_queue->sq.addr,
            .depth = io_queue->depth,
            .qid = io_queue->queue_id,
            .cqid = io_queue->queue_id,
        },
        &sqe
    );
    status = admin_cmd(admin_queue, &sqe, &comp);
    if (!status && !nvme_status_is_success(comp.status))
        status = EIO;
    if (status)
        goto fail_io;

    mutex_lock(&ctrl->admin_lock);
    struct nvme_queue* old_admin = ctrl->admin_queue;
    ctrl->admin_queue = admin_queue;
    mutex_unlock(&ctrl->admin_lock);
    if (old_admin)
        nvme_queue_free(old_admin);

    spin_lock(&ctrl->io_lock);
    ctrl->io_queue = io_queue;
    spin_unlock(&ctrl->io_lock);

    return 0;

fail_io:
    nvme_io_queue_free(io_queue);
fail_admin:
    nvme_queue_free(admin_queue);
    return status;
}

static void trim_ascii_end(char* str, size_t len) {
    while (len > 0 &&
           (str[len - 1] == ' ' || str[len - 1] == '\t' || str[len - 1] == '\n' || str[len - 1] == '\r' ||
            str[len - 1] == '\f'))
        len--;
    str[len] = '\0';
}

errno_t nvme_ctrl_identify(struct nvme_ctrl* ctrl) {
    struct dma_buffer identify_buffer;
    errno_t status = dma_buffer_alloc(ctrl->dma_tag, IDENTIFY_SIZE, &identify_buffer);
    if (status)
        return ENOMEM;

    mutex_lock(&ctrl->admin_lock);
    if (!ctrl->admin_queue) {
        status = ENXIO;
        goto out;
    }

    status = identify(
        ctrl->admin_queue,
        &(struct nvme_identify_cmd){
            .buffer = identify_buffer.addr,
            .controller_id = 0, // TODO: Use a real number here
            .cns = 1,
            .nsid = 0,
        }
    );
    // Check if the identification was successful.
    if (status)
        goto out;

    dma_buffer_sync(&identify_buffer, 0, IDENTIFY_SIZE, DMA_SYNC_POSTREAD);

    // Read the model strings.
    const uint8_t* view = identify_buffer.virt;
    memcpy(ctrl->serial, view + 4, sizeof(ctrl->serial) - 1);
    memcpy(ctrl->model, view + 24, sizeof(ctrl->model) - 1);
    memcpy(ctrl->revision, view + 64, sizeof(ctrl->revision) - 1);
    trim_ascii_end(ctrl->serial, sizeof(ctrl->serial) - 1);
    trim_ascii_end(ctrl->model, sizeof(ctrl->model) - 1);
    trim_ascii_end(ctrl->revision, sizeof(ctrl->revision) - 1);
    log("Model \"%s\"\n", ctrl->model);

    atomic_store(&ctrl->ns_count, mmio_reg_read(identify_buffer.virt, ID_NN), ATOMIC_RELEASE);

    uint8_t mdts = mmio_reg_read(identify_buffer.virt, ID_MDTS);
    if (mdts != 0 && mdts < sizeof(size_t) * 8)
        ctrl->mdts = ((size_t)1 << mdts) * ctrl->mps_min;

out:
    mutex_unlock(&ctrl->admin_lock);
    dma_buffer_free(&identify_buffer);
    return status;
}

// Scans all existing namespaces.
errno_t nvme_ctrl_scan_namespaces(struct nvme_ctrl* ctrl, nvme_ns_vec_t* out) {
    vec_init(out);

    uint32_t* ns_list = mem_alloc(NS_LIST_LEN * sizeof(uint32_t), 0);
    if (!ns_list)
        return ENOMEM;

    struct dma_buffer identify_buffer;
    errno_t status = dma_buffer_alloc(ctrl->dma_tag, IDENTIFY_SIZE, &identify_buffer);
    if (status) {
        mem_free(ns_list, NS_LIST_LEN * sizeof(uint32_t));
        return ENOMEM;
    }

    mutex_lock(&ctrl->admin_lock);
    struct nvme_queue* aq = ctrl->admin_queue;
    if (!aq) {
        status = ENXIO;
        goto out;
    }

    uint32_t nsid = 0;
    while (true) {
        const struct nvme_identify_cmd admin_ident_cmd = {
            .buffer = identify_buffer.addr,
            .controller_id = 0,
            .cns = 2,
            .nsid = nsid,
        };
        status = identify(aq, &admin_ident_cmd);

        if (status)
            goto out;

        dma_buffer_sync(&identify_buffer, 0, IDENTIFY_SIZE, DMA_SYNC_POSTREAD);

        // Parse the identifier list until we hit an namespace value of 0.
        size_t ns_list_len = 0;
        const uint32_t* raw_list = identify_buffer.virt;
        while (ns_list_len < NS_LIST_LEN && raw_list[ns_list_len] != 0) {
            ns_list[ns_list_len] = raw_list[ns_list_len];
            ns_list_len++;
        }

        for (size_t i = 0; i < ns_list_len; i++) {
            const uint32_t id = ns_list[i];
            const struct nvme_identify_cmd ns_ident_cmd = {
                .buffer = identify_buffer.addr,
                .controller_id = 0,
                .cns = 0,
                .nsid = id,
            };
            status = identify(aq, &ns_ident_cmd);

            if (status)
                goto out;

            dma_buffer_sync(&identify_buffer, 0, IDENTIFY_SIZE, DMA_SYNC_POSTREAD);

            const uint64_t lba_count = mmio_reg_read(identify_buffer.virt, ID_NSZE);

            // To get the block size of a namespace, we need to read a descriptor (see LBAF0-15).
            // Get the format descriptor index used by this namespace.
            const uint8_t lba_format_idx = mmio_field_get(ID_LBAF_IDX, mmio_reg_read(identify_buffer.virt, ID_FLBAS));

            const mmio_reg32 lbaf = {ID_LBAF0.offset + lba_format_idx * sizeof(uint32_t), MMIO_LE};
            const uint32_t lba_format = mmio_reg_read(identify_buffer.virt, lbaf);

            uint8_t lba_shift = mmio_field_get(ID_LBADS, lba_format);
            // If LBADS doesn't contain a non-zero value, we need to assume a default. Use 512 bytes here.
            if (lba_shift == 0)
                lba_shift = 9;

            struct nvme_ns* ns;
            status = nvme_ns_new(ctrl, id, lba_shift, lba_count, nvme_ctrl_max_transfer_bytes(ctrl), &ns);
            if (status)
                goto out;
            status = vec_push(out, ns);
            if (status) {
                blockdev_unref(&ns->blk);
                goto out;
            }
        }

        // If there is no 0 in the buffer, there's more entries than can fit in `identify_buffer`.
        if (ns_list_len < NS_LIST_LEN)
            break;
        nsid = ns_list[ns_list_len - 1];
    }

out:
    mutex_unlock(&ctrl->admin_lock);
    dma_buffer_free(&identify_buffer);
    mem_free(ns_list, NS_LIST_LEN * sizeof(uint32_t));
    if (status) {
        vec_foreach(out, i) {
            blockdev_unref(&vec_elem_get(out, i)->blk);
        }
        vec_destroy(out);
    }
    return status;
}

struct nvme_io_queue* nvme_ctrl_io_queue(struct nvme_ctrl* ctrl) {
    spin_lock(&ctrl->io_lock);
    struct nvme_io_queue* queue = ctrl->io_queue;
    spin_unlock(&ctrl->io_lock);
    return queue;
}
