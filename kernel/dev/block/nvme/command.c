// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/nvme/nvme.h>
#include <lib/mmio.h>

void nvme_rw_cmd_write(const struct nvme_rw_cmd* cmd, struct nvme_sqe* sqe) {
    uint32_t cdw0 = mmio_field(NVME_SQE_OPC, cmd->do_write ? NVME_CMD_WRITE : NVME_CMD_READ) |
        mmio_field(NVME_SQE_CID, cmd->cid) | mmio_field(NVME_SQE_PSDT, 0); // We always want to use PRP for this.

    uint32_t cdw10 = mmio_field(NVME_RW_SLBA_LOW, (uint32_t)cmd->start_lba);

    uint32_t cdw11 = mmio_field(NVME_RW_SLBA_HIGH, (uint32_t)(cmd->start_lba >> 32));

    uint32_t cdw12 = mmio_field(NVME_RW_NLB, (uint16_t)(cmd->num_lbas - 1));
    uint32_t cdw13 = 0;
    uint32_t cdw14 = 0;
    uint32_t cdw15 = 0;

    mmio_reg_write(sqe, NVME_SQE_NSID, cmd->nsid);
    mmio_reg_write(sqe, NVME_SQE_DPTR0, cmd->prp1);
    mmio_reg_write(sqe, NVME_SQE_DPTR1, cmd->prp2);
    mmio_reg_write(sqe, NVME_SQE_CDW0, cdw0);
    mmio_reg_write(sqe, NVME_SQE_CDW10, cdw10);
    mmio_reg_write(sqe, NVME_SQE_CDW11, cdw11);
    mmio_reg_write(sqe, NVME_SQE_CDW12, cdw12);
    mmio_reg_write(sqe, NVME_SQE_CDW13, cdw13);
    mmio_reg_write(sqe, NVME_SQE_CDW14, cdw14);
    mmio_reg_write(sqe, NVME_SQE_CDW15, cdw15);
}

void nvme_flush_cmd_write(const struct nvme_flush_cmd* cmd, struct nvme_sqe* sqe) {
    uint32_t cdw0 = mmio_field(NVME_SQE_OPC, NVME_CMD_FLUSH) | mmio_field(NVME_SQE_CID, cmd->cid);

    mmio_reg_write(sqe, NVME_SQE_NSID, cmd->nsid);
    mmio_reg_write(sqe, NVME_SQE_CDW0, cdw0);
}

void nvme_create_cq_cmd_write(const struct nvme_create_cq_cmd* cmd, struct nvme_sqe* sqe) {
    uint32_t cdw0 = mmio_field(NVME_SQE_OPC, NVME_ADMIN_CREATE_CQ) |
        mmio_field(NVME_SQE_PSDT, 0); // We always want to use PRP for this.

    uint32_t cdw10 = mmio_field(NVME_CREATE_CQ_QSIZE, (uint16_t)(cmd->depth - 1)) |
        mmio_field(NVME_CREATE_CQ_QID, (uint16_t)cmd->qid);

    uint32_t cdw11 = mmio_field(NVME_CREATE_CQ_IV, cmd->irq_vector) |
        mmio_field(NVME_CREATE_CQ_IEN, cmd->irqs_enabled ? 1 : 0) |
        mmio_field(NVME_CREATE_CQ_PC, 1); // Our buffer is physically contiguous.

    mmio_reg_write(sqe, NVME_SQE_DPTR0, cmd->cq_addr);
    mmio_reg_write(sqe, NVME_SQE_CDW0, cdw0);
    mmio_reg_write(sqe, NVME_SQE_CDW10, cdw10);
    mmio_reg_write(sqe, NVME_SQE_CDW11, cdw11);
}

void nvme_create_sq_cmd_write(const struct nvme_create_sq_cmd* cmd, struct nvme_sqe* sqe) {
    uint32_t cdw0 = mmio_field(NVME_SQE_OPC, NVME_ADMIN_CREATE_SQ) |
        mmio_field(NVME_SQE_PSDT, 0); // We always want to use PRP for this.

    uint32_t cdw10 = mmio_field(NVME_CREATE_SQ_QSIZE, (uint16_t)(cmd->depth - 1)) |
        mmio_field(NVME_CREATE_SQ_QID, (uint16_t)cmd->qid);

    uint32_t cdw11 = mmio_field(NVME_CREATE_SQ_CQID, (uint16_t)cmd->cqid) |
        mmio_field(NVME_CREATE_SQ_QPRIO, 0) | // Priority is the highest.
        mmio_field(NVME_CREATE_SQ_PC, 1);     // Our buffer is physically contiguous.

    mmio_reg_write(sqe, NVME_SQE_DPTR0, cmd->sq_addr);
    mmio_reg_write(sqe, NVME_SQE_CDW0, cdw0);
    mmio_reg_write(sqe, NVME_SQE_CDW10, cdw10);
    mmio_reg_write(sqe, NVME_SQE_CDW11, cdw11);
}

void nvme_identify_cmd_write(const struct nvme_identify_cmd* cmd, struct nvme_sqe* sqe) {
    uint32_t cdw0 = mmio_field(NVME_SQE_OPC, NVME_ADMIN_IDENTIFY) |
        mmio_field(NVME_SQE_PSDT, 0); // We always want to use PRP for this.

    uint32_t cdw10 = mmio_field(NVME_IDENTIFY_CNTID, cmd->controller_id) | mmio_field(NVME_IDENTIFY_CNS, cmd->cns);

    mmio_reg_write(sqe, NVME_SQE_DPTR0, cmd->buffer);
    mmio_reg_write(sqe, NVME_SQE_CDW0, cdw0);
    mmio_reg_write(sqe, NVME_SQE_NSID, cmd->nsid);
    mmio_reg_write(sqe, NVME_SQE_CDW10, cdw10);
}
