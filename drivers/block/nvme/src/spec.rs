#![allow(unused)]

use zinnia::static_assert;

/// Submission queue entry.
pub mod sq_entry {
    use zinnia::memory::{Field, Register};

    pub const SIZE: usize = 0x40;

    pub const CDW0: Register<u32> = Register::new(0).with_le();

    /// Command Identifier
    pub const CID: Field<u32, u16> = Field::new_bits(CDW0, 16..=31);
    /// PRP or SGL for Data Transfer
    pub const PSDT: Field<u32, u8> = Field::new_bits(CDW0, 14..=15);
    /// Fused Operation
    pub const FUSE: Field<u32, u8> = Field::new_bits(CDW0, 8..=9);
    /// Opcode
    pub const OPC: Field<u32, u8> = Field::new_bits(CDW0, 0..=7);

    /// Namespace Identifier
    pub const NSID: Register<u32> = Register::new(4).with_le();
    /// Metadata Pointer
    pub const MPTR: Register<u64> = Register::new(16).with_le();
    /// Data Pointer 0
    pub const DPTR0: Register<u64> = Register::new(24).with_le();
    /// Data Pointer 1
    pub const DPTR1: Register<u64> = Register::new(32).with_le();

    pub const CDW10: Register<u32> = Register::new(40).with_le();
    pub const CDW11: Register<u32> = Register::new(44).with_le();
    pub const CDW12: Register<u32> = Register::new(48).with_le();
    pub const CDW13: Register<u32> = Register::new(52).with_le();
    pub const CDW14: Register<u32> = Register::new(56).with_le();
    pub const CDW15: Register<u32> = Register::new(60).with_le();

    pub mod create_cq {
        use zinnia::memory::Field;

        pub const OPCODE: u8 = 0x05;

        /// Queue Size
        pub const QSIZE: Field<u32, u16> = Field::new_bits(super::CDW10, 16..=31);
        /// Queue Identifier
        pub const QID: Field<u32, u16> = Field::new_bits(super::CDW10, 0..=15);

        /// Interrupt Vector
        pub const IV: Field<u32, u16> = Field::new_bits(super::CDW11, 16..=31);
        /// Interrupts Enabled
        pub const IEN: Field<u32, u8> = Field::new_bits(super::CDW11, 1..=1);
        /// Physically Contigous
        pub const PC: Field<u32, u8> = Field::new_bits(super::CDW11, 0..=0);
    }

    pub mod create_sq {
        use zinnia::memory::Field;

        pub const OPCODE: u8 = 0x01;

        /// Queue Size
        pub const QSIZE: Field<u32, u16> = Field::new_bits(super::CDW10, 16..=31);
        /// Queue Identifier
        pub const QID: Field<u32, u16> = Field::new_bits(super::CDW10, 0..=15);

        /// Completion Queue Identifier
        pub const CQID: Field<u32, u16> = Field::new_bits(super::CDW11, 16..=31);
        /// Queue Priority
        pub const QPRIO: Field<u32, u8> = Field::new_bits(super::CDW11, 1..=2);
        /// Physically Contigous
        pub const PC: Field<u32, u8> = Field::new_bits(super::CDW11, 0..=0);
    }

    pub mod identify {
        use zinnia::memory::Field;

        /// Controller Identifier
        pub const CNTID: Field<u32, u16> = Field::new_bits(super::CDW10, 16..=31);
        /// Controller or Namespace Structure
        pub const CNS: Field<u32, u8> = Field::new_bits(super::CDW10, 0..=7);
    }

    pub mod rw {
        use zinnia::memory::Field;

        /// Starting LBA Lower Bits
        pub const SLBA_LOW: Field<u32, u32> = Field::new_bits(super::CDW10, 0..=31);
        /// Starting LBA Higher Bits
        pub const SLBA_HIGH: Field<u32, u32> = Field::new_bits(super::CDW11, 0..=31);

        /// Limited Retry
        pub const LR: Field<u32, u8> = Field::new_bits(super::CDW12, 31..=31);
        /// Force Unit Access
        pub const FUA: Field<u32, u8> = Field::new_bits(super::CDW12, 30..=30);
        /// Protection Information Field
        pub const PRINFO: Field<u32, u8> = Field::new_bits(super::CDW12, 26..=29);
        /// Number of Logical Blocks
        pub const NLB: Field<u32, u16> = Field::new_bits(super::CDW12, 0..=15);
    }
}

pub mod cq_entry {
    use zinnia::memory::{Field, Register};

    pub const SIZE: usize = 0x10;

    pub const DW0: Register<u32> = Register::new(0).with_le();
    pub const DW2: Register<u32> = Register::new(8).with_le();

    pub const SQ_IDENT: Field<u32, u16> = Field::new_bits(DW2, 16..=31);
    pub const SQ_HEAD: Field<u32, u16> = Field::new_bits(DW2, 0..=15);

    pub const DW3: Register<u32> = Register::new(12).with_le();

    pub const STATUS: Field<u32, u16> = Field::new_bits(DW3, 17..=31);
    pub const PHASE_TAG: Field<u32, u8> = Field::new_bits(DW3, 16..=16);
    pub const CID: Field<u32, u16> = Field::new_bits(DW3, 0..=15);
}

pub mod regs {
    use zinnia::memory::Register;

    /// Controller Capabilities
    pub const CAP: Register<u64> = Register::new(0x00).with_le();

    pub mod cap {
        use zinnia::memory::Field;

        /// Memory Page Size Maximum
        pub const MPSMAX: Field<u64, u8> = Field::new_bits(super::CAP, 52..=55);
        /// Memory Page Size Minimum
        pub const MPSMIN: Field<u64, u8> = Field::new_bits(super::CAP, 48..=51);
        /// Boot Partition Support
        pub const BPS: Field<u64, u8> = Field::new_bits(super::CAP, 45..=45);
        /// Command Sets Supported
        pub const CSS: Field<u64, u8> = Field::new_bits(super::CAP, 37..=44);
        /// NVM Subsystem Reset Supported
        pub const NSSRS: Field<u64, u8> = Field::new_bits(super::CAP, 36..=36);
        /// Doorbell Stride
        pub const DSTRD: Field<u64, u8> = Field::new_bits(super::CAP, 32..=35);
        /// Timeout
        pub const TO: Field<u64, u8> = Field::new_bits(super::CAP, 24..=31);
        /// Arbitration Mechanism Supported
        pub const AMS: Field<u64, u8> = Field::new_bits(super::CAP, 17..=18);
        /// Contiguous Queues Required
        pub const CQR: Field<u64, u8> = Field::new_bits(super::CAP, 16..=16);
        /// Maximum Queue Entries Supported
        pub const MQES: Field<u64, u16> = Field::new_bits(super::CAP, 0..=15);
    }

    /// Version
    pub const VS: Register<u32> = Register::new(0x08).with_le();

    pub mod vs {
        use zinnia::memory::Field;

        /// Major Version Number
        pub const MJR: Field<u32, u16> = Field::new_bits(super::VS, 16..=31);
        /// Minor Version Number
        pub const MNR: Field<u32, u8> = Field::new_bits(super::VS, 8..=15);
        /// Tertiary Version Number
        pub const TER: Field<u32, u8> = Field::new_bits(super::VS, 0..=7);
    }

    /// Interrupt Mask Set
    pub const INTMS: Register<u32> = Register::new(0x0C).with_le();
    /// Interrupt Mask Clear
    pub const INTMC: Register<u32> = Register::new(0x10).with_le();

    /// Controller Configuration
    pub const CC: Register<u32> = Register::new(0x14).with_le();

    pub mod cc {
        use zinnia::memory::Field;

        /// I/O Completion Queue Entry Size
        pub const IOCQES: Field<u32, u8> = Field::new_bits(super::CC, 20..=23);
        /// I/O Submission Queue Entry Size
        pub const IOSQES: Field<u32, u8> = Field::new_bits(super::CC, 16..=19);
        /// Shutdown Notification
        pub const SHN: Field<u32, u8> = Field::new_bits(super::CC, 14..=15);
        /// Arbitration Mechanism Selected
        pub const AMS: Field<u32, u8> = Field::new_bits(super::CC, 11..=13);
        /// Memory Page Size
        pub const MPS: Field<u32, u8> = Field::new_bits(super::CC, 7..=10);
        /// I/O Command Set Selected
        pub const CSS: Field<u32, u8> = Field::new_bits(super::CC, 4..=6);
        /// Enable
        pub const EN: Field<u32, u8> = Field::new_bits(super::CC, 0..=0);
    }

    /// Controller Status
    pub const CSTS: Register<u32> = Register::new(0x1C).with_le();

    pub mod csts {
        use zinnia::memory::Field;

        /// Processing Paused
        pub const PP: Field<u32, u8> = Field::new_bits(super::CSTS, 5..=5);
        /// NVM Subsystem Reset Occurred
        pub const NSSRO: Field<u32, u8> = Field::new_bits(super::CSTS, 4..=4);
        /// Shutdown Status
        pub const SHST: Field<u32, u8> = Field::new_bits(super::CSTS, 2..=3);
        /// Controller Fatal Status
        pub const CFS: Field<u32, u8> = Field::new_bits(super::CSTS, 1..=1);
        /// Ready
        pub const RDY: Field<u32, u8> = Field::new_bits(super::CSTS, 0..=0);
    }

    /// Admin Queue Attributes
    pub const AQA: Register<u32> = Register::new(0x24).with_le();

    pub mod aqa {
        use zinnia::memory::Field;

        /// Admin Completion Queue Size
        pub const ACQS: Field<u32, u16> = Field::new_bits(super::AQA, 0..=11);
        /// Admin Submission Queue Size
        pub const ASQS: Field<u32, u16> = Field::new_bits(super::AQA, 16..=27);
    }

    /// Admin Submission Queue
    pub const ASQ: Register<u64> = Register::new(0x28).with_le();
    /// Admin Completion Queue
    pub const ACQ: Register<u64> = Register::new(0x30).with_le();
}

/// Generic Command Set
pub mod cmd {
    pub const FLUSH: u8 = 0x00;
    pub const WRITE: u8 = 0x01;
    pub const READ: u8 = 0x02;
}

/// Admin Command Set
pub mod admin_cmd {
    pub const DELETE_SQ: u8 = 0x00;
    pub const CREATE_SQ: u8 = 0x01;
    pub const DELETE_CQ: u8 = 0x04;
    pub const CREATE_CQ: u8 = 0x05;
    pub const IDENTIFY: u8 = 0x06;
    pub const ABORT: u8 = 0x08;
    pub const SET_FEATURES: u8 = 0x09;
    pub const GET_FEATURES: u8 = 0x0A;
}

/// An entry in the completion queue.
#[derive(Clone, Copy, Debug)]
pub struct CompletionEntry {
    pub result: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub cmd_id: u16,
    pub phase_tag: bool,
    pub status: CompletionStatus,
}

/// A status value returned in a [`CompletionEntry`].
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct CompletionStatus(pub u16);

impl CompletionStatus {
    pub fn is_success(&self) -> bool {
        self.0 == 0
    }
}

enum CompletionCode {
    Generic = 0x00,
    CommandSpecific = 0x01,
    MediaAndDataIntegrityError = 0x02,
    PathRelated = 0x03,
    VendorSpecific = 0x07,
}
