#![allow(unused)]

use num_enum::FromPrimitive;
use zinnia::num_enum::TryFromPrimitive;

pub const TRB_SIZE: usize = 0x10;

pub const CONTEXT_SIZE: usize = 32;

pub mod caps {
    use zinnia::memory::{Field, Register};

    pub const CAPLENGTH: Register<u8> = Register::new(0x00);
    pub const HCIVERSION: Register<u16> = Register::new(0x02).with_le();

    pub const HCSPARAMS1: Register<u32> = Register::new(0x04).with_le();
    pub mod hcsparams1 {
        use super::*;

        pub const MAX_SLOTS: Field<u32, u8> = Field::new_bits(HCSPARAMS1, 0..=7);
        pub const MAX_INTRS: Field<u32, u16> = Field::new_bits(HCSPARAMS1, 8..=18);
        pub const MAX_PORTS: Field<u32, u8> = Field::new_bits(HCSPARAMS1, 24..=31);
    }

    pub const HCSPARAMS2: Register<u32> = Register::new(0x08).with_le();
    pub mod hcsparams2 {
        use super::*;

        pub const IST: Field<u32, u8> = Field::new_bits(HCSPARAMS2, 0..=3);
        pub const ERST_MAX: Field<u32, u8> = Field::new_bits(HCSPARAMS2, 4..=7);
        pub const MAX_SCRATCHPAD_HI: Field<u32, u8> = Field::new_bits(HCSPARAMS2, 21..=25);
        pub const SPR: Field<u32, u8> = Field::new_bits(HCSPARAMS2, 26..=26);
        pub const MAX_SCRATCHPAD_LO: Field<u32, u8> = Field::new_bits(HCSPARAMS2, 27..=31);
    }

    pub const HCSPARAMS3: Register<u32> = Register::new(0x0C).with_le();

    pub const HCCPARAMS1: Register<u32> = Register::new(0x10).with_le();
    pub mod hccparams1 {
        use super::*;

        pub const AC64: Field<u32, u8> = Field::new_bits(HCCPARAMS1, 0..=0);
        pub const CSZ: Field<u32, u8> = Field::new_bits(HCCPARAMS1, 2..=2);
        pub const XECP: Field<u32, u16> = Field::new_bits(HCCPARAMS1, 16..=31);
    }

    pub const DBOFF: Register<u32> = Register::new(0x14).with_le();
    pub const RTSOFF: Register<u32> = Register::new(0x18).with_le();
    pub const HCCPARAMS2: Register<u32> = Register::new(0x1C).with_le();
}

pub mod opregs {
    use zinnia::memory::{Field, Register};

    pub const USBCMD: Register<u32> = Register::new(0x00).with_le();
    pub mod usbcmd {
        use super::*;

        pub const RS: Field<u32, u8> = Field::new_bits(USBCMD, 0..=0);
        pub const HCRST: Field<u32, u8> = Field::new_bits(USBCMD, 1..=1);
        pub const INTE: Field<u32, u8> = Field::new_bits(USBCMD, 2..=2);
    }

    pub const USBSTS: Register<u32> = Register::new(0x04).with_le();
    pub mod usbsts {
        use super::*;

        pub const HCH: Field<u32, u8> = Field::new_bits(USBSTS, 0..=0);
        pub const EINT: Field<u32, u8> = Field::new_bits(USBSTS, 3..=3);
        pub const CNR: Field<u32, u8> = Field::new_bits(USBSTS, 11..=11);
        pub const HCE: Field<u32, u8> = Field::new_bits(USBSTS, 12..=12);
    }

    pub const PAGESIZE: Register<u32> = Register::new(0x08).with_le();
    pub const DNCTRL: Register<u32> = Register::new(0x14).with_le();

    pub const CRCR: Register<u64> = Register::new(0x18).with_le();
    pub mod crcr {
        use super::*;

        pub const RCS: Field<u64, u8> = Field::new_bits(CRCR, 0..=0);
        pub const CS: Field<u64, u8> = Field::new_bits(CRCR, 1..=1);
        pub const CA: Field<u64, u8> = Field::new_bits(CRCR, 2..=2);
        pub const CRR: Field<u64, u8> = Field::new_bits(CRCR, 3..=3);
    }

    pub const DCBAAP: Register<u64> = Register::new(0x30).with_le();

    pub const CONFIG: Register<u32> = Register::new(0x38).with_le();
    pub mod config {
        use super::*;

        pub const MAX_SLOTS_EN: Field<u32, u8> = Field::new_bits(CONFIG, 0..=7);
    }

    pub const PORT_REGS_BASE: usize = 0x400;
}

pub mod port {
    use zinnia::memory::{Field, Register};

    pub const STRIDE: usize = 0x10;

    pub const PORTSC: Register<u32> = Register::new(0x00).with_le();
    pub mod portsc {
        use super::*;

        pub const CCS: Field<u32, u8> = Field::new_bits(PORTSC, 0..=0);
        pub const PED: Field<u32, u8> = Field::new_bits(PORTSC, 1..=1);
        pub const OCA: Field<u32, u8> = Field::new_bits(PORTSC, 3..=3);
        pub const PR: Field<u32, u8> = Field::new_bits(PORTSC, 4..=4);
        pub const PLS: Field<u32, u8> = Field::new_bits(PORTSC, 5..=8);
        pub const PP: Field<u32, u8> = Field::new_bits(PORTSC, 9..=9);
        pub const SPEED: Field<u32, u8> = Field::new_bits(PORTSC, 10..=13);
        pub const CSC: Field<u32, u8> = Field::new_bits(PORTSC, 17..=17);
        pub const PEC: Field<u32, u8> = Field::new_bits(PORTSC, 18..=18);
        pub const WRC: Field<u32, u8> = Field::new_bits(PORTSC, 19..=19);
        pub const OCC: Field<u32, u8> = Field::new_bits(PORTSC, 20..=20);
        pub const PRC: Field<u32, u8> = Field::new_bits(PORTSC, 21..=21);
        pub const PLC: Field<u32, u8> = Field::new_bits(PORTSC, 22..=22);
        pub const CEC: Field<u32, u8> = Field::new_bits(PORTSC, 23..=23);
        pub const CAS: Field<u32, u8> = Field::new_bits(PORTSC, 24..=24);
        pub const DR: Field<u32, u8> = Field::new_bits(PORTSC, 30..=30);
        pub const WPR: Field<u32, u8> = Field::new_bits(PORTSC, 31..=31);
    }

    pub mod portsc_bits {
        pub const CCS: u32 = 1 << 0;
        pub const PED: u32 = 1 << 1;
        pub const OCA: u32 = 1 << 3;
        pub const PR: u32 = 1 << 4;
        pub const PP: u32 = 1 << 9;
        pub const CSC: u32 = 1 << 17;
        pub const PEC: u32 = 1 << 18;
        pub const WRC: u32 = 1 << 19;
        pub const OCC: u32 = 1 << 20;
        pub const PRC: u32 = 1 << 21;
        pub const PLC: u32 = 1 << 22;
        pub const CEC: u32 = 1 << 23;
        pub const CAS: u32 = 1 << 24;
        pub const DR: u32 = 1 << 30;
        pub const WPR: u32 = 1 << 31;

        pub const CHANGE_BITS: u32 = CSC | PEC | WRC | OCC | PRC | PLC | CEC;
        pub const PRESERVE_BITS: u32 = PP;
    }

    pub const PORTPMSC: Register<u32> = Register::new(0x04).with_le();
    pub const PORTLI: Register<u32> = Register::new(0x08).with_le();
    pub const PORTHLPMC: Register<u32> = Register::new(0x0C).with_le();
}

pub mod runtime {
    use zinnia::memory::Register;

    pub const MFINDEX: Register<u32> = Register::new(0x00).with_le();

    pub const INTERRUPTER_BASE: usize = 0x20;
}

pub mod interrupter {
    use zinnia::memory::{Field, Register};

    pub const STRIDE: usize = 0x20;

    pub const IMAN: Register<u32> = Register::new(0x00).with_le();
    pub mod iman {
        use super::*;

        pub const IP: Field<u32, u8> = Field::new_bits(IMAN, 0..=0);
        pub const IE: Field<u32, u8> = Field::new_bits(IMAN, 1..=1);
    }

    pub const IMOD: Register<u32> = Register::new(0x04).with_le();
    pub const ERSTSZ: Register<u32> = Register::new(0x08).with_le();
    pub const ERSTBA: Register<u64> = Register::new(0x10).with_le();

    pub const ERDP: Register<u64> = Register::new(0x18).with_le();
    pub mod erdp {
        use super::*;

        pub const DESI: Field<u64, u8> = Field::new_bits(ERDP, 0..=2);
        pub const EHB: Field<u64, u8> = Field::new_bits(ERDP, 3..=3);
    }
}

pub mod doorbell {
    use zinnia::memory::{Field, Register};

    pub const STRIDE: usize = 0x04;

    pub const DOORBELL: Register<u32> = Register::new(0x00).with_le();

    pub const TARGET: Field<u32, u8> = Field::new_bits(DOORBELL, 0..=7);
    pub const STREAM_ID: Field<u32, u16> = Field::new_bits(DOORBELL, 16..=31);
}

pub mod erst_entry {
    use zinnia::memory::Register;

    pub const SIZE: usize = 0x10;

    pub const RING_SEGMENT_BASE: Register<u64> = Register::new(0x00).with_le();
    pub const RING_SEGMENT_SIZE: Register<u16> = Register::new(0x08).with_le();
}

pub mod trb {
    use zinnia::memory::{Field, Register};

    pub const PARAMETER: Register<u64> = Register::new(0x00).with_le();

    pub const STATUS: Register<u32> = Register::new(0x08).with_le();
    pub mod status {
        use super::*;

        pub const TRANSFER_LEN: Field<u32, u32> = Field::new_bits(STATUS, 0..=16);
        pub const TD_SIZE: Field<u32, u8> = Field::new_bits(STATUS, 17..=21);
        pub const INTERRUPTER_TARGET: Field<u32, u16> = Field::new_bits(STATUS, 22..=31);

        pub const COMPLETION_CODE: Field<u32, u8> = Field::new_bits(STATUS, 24..=31);
        pub const COMPLETION_RESIDUE: Field<u32, u32> = Field::new_bits(STATUS, 0..=23);
    }

    pub const CONTROL: Register<u32> = Register::new(0x0C).with_le();
    pub mod control {
        use super::*;

        pub const C: Field<u32, u8> = Field::new_bits(CONTROL, 0..=0);
        pub const TC: Field<u32, u8> = Field::new_bits(CONTROL, 1..=1);
        pub const ISP: Field<u32, u8> = Field::new_bits(CONTROL, 2..=2);
        pub const CH: Field<u32, u8> = Field::new_bits(CONTROL, 4..=4);
        pub const IOC: Field<u32, u8> = Field::new_bits(CONTROL, 5..=5);
        pub const IDT: Field<u32, u8> = Field::new_bits(CONTROL, 6..=6);
        pub const TRB_TYPE: Field<u32, u8> = Field::new_bits(CONTROL, 10..=15);
        pub const DIR: Field<u32, u8> = Field::new_bits(CONTROL, 16..=16);
        pub const TRT: Field<u32, u8> = Field::new_bits(CONTROL, 16..=17);
        pub const SLOT_ID: Field<u32, u8> = Field::new_bits(CONTROL, 24..=31);
        pub const ENDPOINT_ID: Field<u32, u8> = Field::new_bits(CONTROL, 16..=20);
    }
}

pub mod slot_ctx {
    use zinnia::memory::{Field, Register};

    pub const DW0: Register<u32> = Register::new(0x00).with_le();
    pub const ROUTE_STRING: Field<u32, u32> = Field::new_bits(DW0, 0..=19);
    pub const SPEED: Field<u32, u8> = Field::new_bits(DW0, 20..=23);
    pub const MTT: Field<u32, u8> = Field::new_bits(DW0, 25..=25);
    pub const HUB: Field<u32, u8> = Field::new_bits(DW0, 26..=26);
    pub const CTX_ENTRIES: Field<u32, u8> = Field::new_bits(DW0, 27..=31);

    pub const DW1: Register<u32> = Register::new(0x04).with_le();
    pub const MAX_EXIT_LATENCY: Field<u32, u16> = Field::new_bits(DW1, 0..=15);
    pub const ROOT_HUB_PORT_NUMBER: Field<u32, u8> = Field::new_bits(DW1, 16..=23);
    pub const NUMBER_OF_PORTS: Field<u32, u8> = Field::new_bits(DW1, 24..=31);

    pub const DW2: Register<u32> = Register::new(0x08).with_le();
    pub const TT_HUB_SLOT_ID: Field<u32, u8> = Field::new_bits(DW2, 0..=7);
    pub const TT_PORT_NUMBER: Field<u32, u8> = Field::new_bits(DW2, 8..=15);
    pub const TTT: Field<u32, u8> = Field::new_bits(DW2, 16..=17);
    pub const INTERRUPTER_TARGET: Field<u32, u16> = Field::new_bits(DW2, 22..=31);

    pub const DW3: Register<u32> = Register::new(0x0C).with_le();
    pub const USB_DEVICE_ADDRESS: Field<u32, u8> = Field::new_bits(DW3, 0..=7);
    pub const SLOT_STATE: Field<u32, u8> = Field::new_bits(DW3, 27..=31);
}

pub mod ep_ctx {
    use zinnia::memory::{Field, Register};

    pub const DW0: Register<u32> = Register::new(0x00).with_le();
    pub const EP_STATE: Field<u32, u8> = Field::new_bits(DW0, 0..=2);
    pub const MULT: Field<u32, u8> = Field::new_bits(DW0, 8..=9);
    pub const MAX_PSTREAMS: Field<u32, u8> = Field::new_bits(DW0, 10..=14);
    pub const LSA: Field<u32, u8> = Field::new_bits(DW0, 15..=15);
    pub const INTERVAL: Field<u32, u8> = Field::new_bits(DW0, 16..=23);
    pub const MAX_ESIT_PAYLOAD_HI: Field<u32, u8> = Field::new_bits(DW0, 24..=31);

    pub const DW1: Register<u32> = Register::new(0x04).with_le();
    pub const CERR: Field<u32, u8> = Field::new_bits(DW1, 1..=2);
    pub const EP_TYPE: Field<u32, u8> = Field::new_bits(DW1, 3..=5);
    pub const HID: Field<u32, u8> = Field::new_bits(DW1, 7..=7);
    pub const MAX_BURST_SIZE: Field<u32, u8> = Field::new_bits(DW1, 8..=15);
    pub const MAX_PACKET_SIZE: Field<u32, u16> = Field::new_bits(DW1, 16..=31);

    pub const DW2: Register<u32> = Register::new(0x08).with_le();
    pub const DCS: Field<u32, u8> = Field::new_bits(DW2, 0..=0);
    pub const TR_DEQUEUE_PTR_LO: Field<u32, u32> = Field::new_bits(DW2, 4..=31);

    pub const DW3: Register<u32> = Register::new(0x0C).with_le();
    pub const TR_DEQUEUE_PTR_HI: Field<u32, u32> = Field::new_bits(DW3, 0..=31);

    pub const TR_DEQUEUE_PTR: Register<u64> = Register::new(0x08).with_le();

    pub const DW4: Register<u32> = Register::new(0x10).with_le();
    pub const AVG_TRB_LENGTH: Field<u32, u16> = Field::new_bits(DW4, 0..=15);
    pub const MAX_ESIT_PAYLOAD_LO: Field<u32, u16> = Field::new_bits(DW4, 16..=31);
}

pub mod input_ctx {
    use zinnia::memory::{Field, Register};

    pub const DROP_FLAGS: Register<u32> = Register::new(0x00).with_le();
    pub const ADD_FLAGS: Register<u32> = Register::new(0x04).with_le();

    pub const DW7: Register<u32> = Register::new(0x1C).with_le();
    pub const CONFIGURATION_VALUE: Field<u32, u8> = Field::new_bits(DW7, 0..=7);
    pub const INTERFACE_NUMBER: Field<u32, u8> = Field::new_bits(DW7, 8..=15);
    pub const ALTERNATE_SETTING: Field<u32, u8> = Field::new_bits(DW7, 16..=23);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Link = 6,
    EnableSlot = 9,
    DisableSlot = 10,
    AddressDevice = 11,
    ConfigureEndpoint = 12,
    EvaluateContext = 13,
    ResetEndpoint = 14,
    StopEndpoint = 15,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompletionCode {
    Invalid = 0,
    Success = 1,
    DataBufferError = 2,
    BabbleDetected = 3,
    TransactionError = 4,
    TrbError = 5,
    Stall = 6,
    ResourceError = 7,
    BandwidthError = 8,
    NoSlotsAvailable = 9,
    InvalidStreamType = 10,
    SlotNotEnabled = 11,
    EndpointNotEnabled = 12,
    ShortPacket = 13,
    RingUnderrun = 14,
    RingOverrun = 15,
    VfEventRingFull = 16,
    ParameterError = 17,
    BandwidthOverrunError = 18,
    ContextStateError = 19,
    NoPingResponse = 20,
    EventRingFull = 21,
    IncompatibleDeviceError = 22,
    MissedService = 23,
    CommandRingStopped = 24,
    CommandRingAborted = 25,
    Stopped = 26,
    StoppedLength = 27,
    StoppedShort = 28,
    LatencyTooLarge = 29,
    BufferOverrun = 31,
    EventLost = 32,
    UndefinedError = 33,
    InvalidStreamId = 34,
    SecondaryBandwidthError = 35,
    SplitTransactionError = 36,
}

impl CompletionCode {
    pub fn is_success(self) -> bool {
        matches!(self, Self::Success | Self::ShortPacket)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum PortSpeed {
    Full = 1,
    Low = 2,
    High = 3,
    Super = 4,
    SuperSpeedPlus2x1 = 5,
    SuperSpeedPlus1x2 = 6,
    SuperSpeedPlus2x2 = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EndpointType {
    IsochOut = 1,
    BulkOut = 2,
    InterruptOut = 3,
    Control = 4,
    IsochIn = 5,
    BulkIn = 6,
    InterruptIn = 7,
}
