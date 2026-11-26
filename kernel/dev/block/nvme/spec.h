// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <mem/mmio.h>
#include <stddef.h>
#include <stdint.h>

// Submission queue entry.
struct [[__packed]] nvme_sqe {
    uint32_t cdw0;

    uint32_t nsid; // Namespace Identifier
    uint32_t cdw2;
    uint32_t cdw3;

    uint64_t mptr; // Metadata Pointer

    uint64_t dptr0; // Data Pointer 0
    uint64_t dptr1; // Data Pointer 1

    uint32_t cdw10;
    uint32_t cdw11;
    uint32_t cdw12;
    uint32_t cdw13;
    uint32_t cdw14;
    uint32_t cdw15;
};
static_assert(sizeof(struct nvme_sqe) == 0x40);

constexpr mmio_reg32 NVME_SQE_CDW0 = {offsetof(struct nvme_sqe, cdw0), MMIO_LE};
constexpr mmio_field32 NVME_SQE_CID = {NVME_SQE_CDW0, 16, 16}; // Command Identifier
constexpr mmio_field32 NVME_SQE_PSDT = {NVME_SQE_CDW0, 14, 2}; // PRP or SGL for Data Transfer
constexpr mmio_field32 NVME_SQE_FUSE = {NVME_SQE_CDW0, 8, 2};  // Fused Operation
constexpr mmio_field32 NVME_SQE_OPC = {NVME_SQE_CDW0, 0, 8};   // Opcode

constexpr mmio_reg32 NVME_SQE_NSID = {offsetof(struct nvme_sqe, nsid), MMIO_LE};   // Namespace Identifier
constexpr mmio_reg64 NVME_SQE_MPTR = {offsetof(struct nvme_sqe, mptr), MMIO_LE};   // Metadata Pointer
constexpr mmio_reg64 NVME_SQE_DPTR0 = {offsetof(struct nvme_sqe, dptr0), MMIO_LE}; // Data Pointer 0
constexpr mmio_reg64 NVME_SQE_DPTR1 = {offsetof(struct nvme_sqe, dptr1), MMIO_LE}; // Data Pointer 1

constexpr mmio_reg32 NVME_SQE_CDW10 = {offsetof(struct nvme_sqe, cdw10), MMIO_LE};
constexpr mmio_reg32 NVME_SQE_CDW11 = {offsetof(struct nvme_sqe, cdw11), MMIO_LE};
constexpr mmio_reg32 NVME_SQE_CDW12 = {offsetof(struct nvme_sqe, cdw12), MMIO_LE};
constexpr mmio_reg32 NVME_SQE_CDW13 = {offsetof(struct nvme_sqe, cdw13), MMIO_LE};
constexpr mmio_reg32 NVME_SQE_CDW14 = {offsetof(struct nvme_sqe, cdw14), MMIO_LE};
constexpr mmio_reg32 NVME_SQE_CDW15 = {offsetof(struct nvme_sqe, cdw15), MMIO_LE};

constexpr uint8_t NVME_CREATE_CQ_OPCODE = 0x05;
constexpr mmio_field32 NVME_CREATE_CQ_QSIZE = {NVME_SQE_CDW10, 16, 16}; // Queue Size
constexpr mmio_field32 NVME_CREATE_CQ_QID = {NVME_SQE_CDW10, 0, 16};    // Queue Identifier
constexpr mmio_field32 NVME_CREATE_CQ_IV = {NVME_SQE_CDW11, 16, 16};    // Interrupt Vector
constexpr mmio_field32 NVME_CREATE_CQ_IEN = {NVME_SQE_CDW11, 1, 1};     // Interrupts Enabled
constexpr mmio_field32 NVME_CREATE_CQ_PC = {NVME_SQE_CDW11, 0, 1};      // Physically Contiguous

constexpr uint8_t NVME_CREATE_SQ_OPCODE = 0x01;
constexpr mmio_field32 NVME_CREATE_SQ_QSIZE = {NVME_SQE_CDW10, 16, 16}; // Queue Size
constexpr mmio_field32 NVME_CREATE_SQ_QID = {NVME_SQE_CDW10, 0, 16};    // Queue Identifier
constexpr mmio_field32 NVME_CREATE_SQ_CQID = {NVME_SQE_CDW11, 16, 16};  // Completion Queue Identifier
constexpr mmio_field32 NVME_CREATE_SQ_QPRIO = {NVME_SQE_CDW11, 1, 2};   // Queue Priority
constexpr mmio_field32 NVME_CREATE_SQ_PC = {NVME_SQE_CDW11, 0, 1};      // Physically Contiguous

constexpr mmio_field32 NVME_IDENTIFY_CNTID = {NVME_SQE_CDW10, 16, 16}; // Controller Identifier
constexpr mmio_field32 NVME_IDENTIFY_CNS = {NVME_SQE_CDW10, 0, 8};     // Controller or Namespace Structure

constexpr mmio_field32 NVME_RW_SLBA_LOW = {NVME_SQE_CDW10, 0, 32};  // Starting LBA Lower Bits
constexpr mmio_field32 NVME_RW_SLBA_HIGH = {NVME_SQE_CDW11, 0, 32}; // Starting LBA Higher Bits
constexpr mmio_field32 NVME_RW_LR = {NVME_SQE_CDW12, 31, 1};        // Limited Retry
constexpr mmio_field32 NVME_RW_FUA = {NVME_SQE_CDW12, 30, 1};       // Force Unit Access
constexpr mmio_field32 NVME_RW_PRINFO = {NVME_SQE_CDW12, 26, 4};    // Protection Information Field
constexpr mmio_field32 NVME_RW_NLB = {NVME_SQE_CDW12, 0, 16};       // Number of Logical Blocks

struct [[__packed]] nvme_cqe {
    uint32_t dw0;
    uint32_t dw1;
    uint32_t dw2;
    uint32_t dw3;
};
static_assert(sizeof(struct nvme_cqe) == 0x10);

constexpr mmio_reg32 NVME_CQE_DW0 = {offsetof(struct nvme_cqe, dw0), MMIO_LE};
constexpr mmio_reg32 NVME_CQE_DW2 = {offsetof(struct nvme_cqe, dw2), MMIO_LE};
constexpr mmio_field32 NVME_CQE_SQ_IDENT = {NVME_CQE_DW2, 16, 16};
constexpr mmio_field32 NVME_CQE_SQ_HEAD = {NVME_CQE_DW2, 0, 16};
constexpr mmio_reg32 NVME_CQE_DW3 = {offsetof(struct nvme_cqe, dw3), MMIO_LE};
constexpr mmio_field32 NVME_CQE_STATUS = {NVME_CQE_DW3, 17, 15};
constexpr mmio_field32 NVME_CQE_PHASE_TAG = {NVME_CQE_DW3, 16, 1};
constexpr mmio_field32 NVME_CQE_CID = {NVME_CQE_DW3, 0, 16};

constexpr mmio_reg64 NVME_REG_CAP = {0x00, MMIO_LE};            // Controller Capabilities
constexpr mmio_field64 NVME_CAP_MPSMAX = {NVME_REG_CAP, 52, 4}; // Memory Page Size Maximum
constexpr mmio_field64 NVME_CAP_MPSMIN = {NVME_REG_CAP, 48, 4}; // Memory Page Size Minimum
constexpr mmio_field64 NVME_CAP_BPS = {NVME_REG_CAP, 45, 1};    // Boot Partition Support
constexpr mmio_field64 NVME_CAP_CSS = {NVME_REG_CAP, 37, 8};    // Command Sets Supported
constexpr mmio_field64 NVME_CAP_NSSRS = {NVME_REG_CAP, 36, 1};  // NVM Subsystem Reset Supported
constexpr mmio_field64 NVME_CAP_DSTRD = {NVME_REG_CAP, 32, 4};  // Doorbell Stride
constexpr mmio_field64 NVME_CAP_TO = {NVME_REG_CAP, 24, 8};     // Timeout
constexpr mmio_field64 NVME_CAP_AMS = {NVME_REG_CAP, 17, 2};    // Arbitration Mechanism Supported
constexpr mmio_field64 NVME_CAP_CQR = {NVME_REG_CAP, 16, 1};    // Contiguous Queues Required
constexpr mmio_field64 NVME_CAP_MQES = {NVME_REG_CAP, 0, 16};   // Maximum Queue Entries Supported

constexpr mmio_reg32 NVME_REG_VS = {0x08, MMIO_LE};         // Version
constexpr mmio_field32 NVME_VS_MJR = {NVME_REG_VS, 16, 16}; // Major Version Number
constexpr mmio_field32 NVME_VS_MNR = {NVME_REG_VS, 8, 8};   // Minor Version Number
constexpr mmio_field32 NVME_VS_TER = {NVME_REG_VS, 0, 8};   // Tertiary Version Number

constexpr mmio_reg32 NVME_REG_INTMS = {0x0C, MMIO_LE}; // Interrupt Mask Set
constexpr mmio_reg32 NVME_REG_INTMC = {0x10, MMIO_LE}; // Interrupt Mask Clear

constexpr mmio_reg32 NVME_REG_CC = {0x14, MMIO_LE};           // Controller Configuration
constexpr mmio_field32 NVME_CC_IOCQES = {NVME_REG_CC, 20, 4}; // I/O Completion Queue Entry Size
constexpr mmio_field32 NVME_CC_IOSQES = {NVME_REG_CC, 16, 4}; // I/O Submission Queue Entry Size
constexpr mmio_field32 NVME_CC_SHN = {NVME_REG_CC, 14, 2};    // Shutdown Notification
constexpr mmio_field32 NVME_CC_AMS = {NVME_REG_CC, 11, 3};    // Arbitration Mechanism Selected
constexpr mmio_field32 NVME_CC_MPS = {NVME_REG_CC, 7, 4};     // Memory Page Size
constexpr mmio_field32 NVME_CC_CSS = {NVME_REG_CC, 4, 3};     // I/O Command Set Selected
constexpr mmio_field32 NVME_CC_EN = {NVME_REG_CC, 0, 1};      // Enable

constexpr mmio_reg32 NVME_REG_CSTS = {0x1C, MMIO_LE};           // Controller Status
constexpr mmio_field32 NVME_CSTS_PP = {NVME_REG_CSTS, 5, 1};    // Processing Paused
constexpr mmio_field32 NVME_CSTS_NSSRO = {NVME_REG_CSTS, 4, 1}; // NVM Subsystem Reset Occurred
constexpr mmio_field32 NVME_CSTS_SHST = {NVME_REG_CSTS, 2, 2};  // Shutdown Status
constexpr mmio_field32 NVME_CSTS_CFS = {NVME_REG_CSTS, 1, 1};   // Controller Fatal Status
constexpr mmio_field32 NVME_CSTS_RDY = {NVME_REG_CSTS, 0, 1};   // Ready

constexpr mmio_reg32 NVME_REG_AQA = {0x24, MMIO_LE}; // Admin Queue Attributes

constexpr mmio_field32 NVME_AQA_ACQS = {NVME_REG_AQA, 0, 12};  // Admin Completion Queue Size
constexpr mmio_field32 NVME_AQA_ASQS = {NVME_REG_AQA, 16, 12}; // Admin Submission Queue Size

constexpr mmio_reg64 NVME_REG_ASQ = {0x28, MMIO_LE}; // Admin Submission Queue
constexpr mmio_reg64 NVME_REG_ACQ = {0x30, MMIO_LE}; // Admin Completion Queue

// Generic Command Set
enum nvme_cmd : uint8_t {
    NVME_CMD_FLUSH = 0x00,
    NVME_CMD_WRITE = 0x01,
    NVME_CMD_READ = 0x02,
};

// Admin Command Set
enum nvme_admin_cmd : uint8_t {
    NVME_ADMIN_DELETE_SQ = 0x00,
    NVME_ADMIN_CREATE_SQ = 0x01,
    NVME_ADMIN_DELETE_CQ = 0x04,
    NVME_ADMIN_CREATE_CQ = 0x05,
    NVME_ADMIN_IDENTIFY = 0x06,
    NVME_ADMIN_ABORT = 0x08,
    NVME_ADMIN_SET_FEATURES = 0x09,
    NVME_ADMIN_GET_FEATURES = 0x0A,
};
