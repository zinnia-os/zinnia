use zinnia::memory::{Field, Register};

pub const CTRL: Register<u32> = Register::new(0x0000).with_le();
pub mod ctrl {
    use super::*;

    pub const GIO_MASTER_DISABLE: Field<u32, u8> = Field::new_bits(CTRL, 2..=2);

    pub const SLU: Field<u32, u8> = Field::new_bits(CTRL, 6..=6);

    pub const FRCSPD: Field<u32, u8> = Field::new_bits(CTRL, 11..=11);

    pub const FRCDPX: Field<u32, u8> = Field::new_bits(CTRL, 12..=12);

    pub const RST: Field<u32, u8> = Field::new_bits(CTRL, 26..=26);
}

pub const STATUS: Register<u32> = Register::new(0x0008).with_le();
pub mod status {
    use super::*;

    pub const FD: Field<u32, u8> = Field::new_bits(STATUS, 0..=0);

    pub const LU: Field<u32, u8> = Field::new_bits(STATUS, 1..=1);

    pub const SPEED: Field<u32, u8> = Field::new_bits(STATUS, 6..=7);

    pub const GIO_MASTER_ENABLE: Field<u32, u8> = Field::new_bits(STATUS, 19..=19);

    pub const SPEED_2500: Field<u32, u8> = Field::new_bits(STATUS, 22..=22);
}

pub const EECD: Register<u32> = Register::new(0x0010).with_le();
pub mod eecd {
    use super::*;

    pub const AUTO_RD: Field<u32, u8> = Field::new_bits(EECD, 9..=9);
}

pub const MDIC: Register<u32> = Register::new(0x0020).with_le();
pub mod mdic {
    use super::*;
    pub const DATA: Field<u32, u16> = Field::new_bits(MDIC, 0..=15);
    pub const REGADD: Field<u32, u8> = Field::new_bits(MDIC, 16..=20);
    pub const OP: Field<u32, u8> = Field::new_bits(MDIC, 26..=27);
    pub const READY: Field<u32, u8> = Field::new_bits(MDIC, 28..=28);
    pub const ERROR: Field<u32, u8> = Field::new_bits(MDIC, 30..=30);

    pub const OP_WRITE: u8 = 1;
    pub const OP_READ: u8 = 2;
}

pub const RCTL: Register<u32> = Register::new(0x0100).with_le();
pub mod rctl {
    use super::*;
    pub const EN: Field<u32, u8> = Field::new_bits(RCTL, 1..=1);

    pub const BAM: Field<u32, u8> = Field::new_bits(RCTL, 15..=15);

    pub const SECRC: Field<u32, u8> = Field::new_bits(RCTL, 26..=26);
}

pub const TCTL: Register<u32> = Register::new(0x0400).with_le();
pub mod tctl {
    use super::*;
    pub const EN: Field<u32, u8> = Field::new_bits(TCTL, 1..=1);

    pub const PSP: Field<u32, u8> = Field::new_bits(TCTL, 3..=3);

    pub const CT: Field<u32, u8> = Field::new_bits(TCTL, 4..=11);

    pub const RTLC: Field<u32, u8> = Field::new_bits(TCTL, 24..=24);
}
pub const COLLISION_THRESHOLD: u8 = 15;

pub const ICR: Register<u32> = Register::new(0x1500).with_le();

pub mod icr {
    use super::*;

    pub const LSC: Field<u32, u8> = Field::new_bits(ICR, 2..=2);
}

pub const IMS: Register<u32> = Register::new(0x1508).with_le();
pub mod ims {
    use super::*;
    pub const LSC: Field<u32, u8> = Field::new_bits(IMS, 2..=2);
}

pub const IMC: Register<u32> = Register::new(0x150C).with_le();

pub const GPIE: Register<u32> = Register::new(0x1514).with_le();
pub mod gpie {
    use super::*;

    pub const NSICR: Field<u32, u8> = Field::new_bits(GPIE, 0..=0);
    pub const MSIX_MODE: Field<u32, u8> = Field::new_bits(GPIE, 4..=4);

    pub const EIAME: Field<u32, u8> = Field::new_bits(GPIE, 30..=30);

    pub const PBA: Field<u32, u8> = Field::new_bits(GPIE, 31..=31);
}

pub const EIMS: Register<u32> = Register::new(0x1524).with_le();
pub const EIAC: Register<u32> = Register::new(0x152C).with_le();
pub const EIAM: Register<u32> = Register::new(0x1530).with_le();

pub const EITR0: Register<u32> = Register::new(0x1680).with_le();

pub const START_ITR: u32 = 648;

pub const IVAR0: Register<u32> = Register::new(0x1700).with_le();
pub mod ivar0 {
    use super::*;

    pub const RX_Q0: Field<u32, u8> = Field::new_bits(IVAR0, 0..=7);

    pub const TX_Q0: Field<u32, u8> = Field::new_bits(IVAR0, 8..=15);
}

pub const IVAR_MISC: Register<u32> = Register::new(0x1740).with_le();

pub mod ivar_misc {
    use super::*;
    pub const OTHER: Field<u32, u8> = Field::new_bits(IVAR_MISC, 8..=15);
}

pub const IVAR_VALID: u8 = 0x80;

pub const RAR_COUNT: usize = 16;
pub const MTA_COUNT: usize = 128;

pub const fn ral(n: usize) -> Register<u32> {
    Register::new(0x5400 + 8 * n).with_le()
}
pub const fn rah(n: usize) -> Register<u32> {
    Register::new(0x5404 + 8 * n).with_le()
}

pub const RAL0: Register<u32> = ral(0);
pub const RAH0: Register<u32> = rah(0);

pub mod rah0 {
    use super::*;

    pub const AV: Field<u32, u8> = Field::new_bits(RAH0, 31..=31);
}

pub const fn mta(i: usize) -> Register<u32> {
    Register::new(0x5200 + 4 * i).with_le()
}

pub const SWSM: Register<u32> = Register::new(0x5B50).with_le();

pub mod swsm {
    use super::*;

    pub const SMBI: Field<u32, u8> = Field::new_bits(SWSM, 0..=0);

    pub const SWESMBI: Field<u32, u8> = Field::new_bits(SWSM, 1..=1);
}

pub const SW_FW_SYNC: Register<u32> = Register::new(0x5B5C).with_le();
pub const SWFW_PHY0_SM: u32 = 0x2;

pub const RDBAL0: Register<u32> = Register::new(0xC000).with_le();
pub const RDBAH0: Register<u32> = Register::new(0xC004).with_le();
pub const RDLEN0: Register<u32> = Register::new(0xC008).with_le();
pub const RDH0: Register<u32> = Register::new(0xC010).with_le();
pub const RDT0: Register<u32> = Register::new(0xC018).with_le();
pub const TDBAL0: Register<u32> = Register::new(0xE000).with_le();
pub const TDBAH0: Register<u32> = Register::new(0xE004).with_le();
pub const TDLEN0: Register<u32> = Register::new(0xE008).with_le();
pub const TDH0: Register<u32> = Register::new(0xE010).with_le();
pub const TDT0: Register<u32> = Register::new(0xE018).with_le();

pub const SRRCTL0: Register<u32> = Register::new(0xC00C).with_le();

pub mod srrctl0 {
    use super::*;

    pub const BSIZEPKT: Field<u32, u8> = Field::new_bits(SRRCTL0, 0..=6);

    pub const BSIZEHDR: Field<u32, u8> = Field::new_bits(SRRCTL0, 8..=13);

    pub const DESCTYPE: Field<u32, u8> = Field::new_bits(SRRCTL0, 25..=27);
    pub const DESCTYPE_ADV_ONEBUF: u8 = 1;
}

pub const RXDCTL0: Register<u32> = Register::new(0xC028).with_le();

pub mod rxdctl0 {
    use super::*;
    pub const PTHRESH: Field<u32, u8> = Field::new_bits(RXDCTL0, 0..=4);
    pub const HTHRESH: Field<u32, u8> = Field::new_bits(RXDCTL0, 8..=12);
    pub const WTHRESH: Field<u32, u8> = Field::new_bits(RXDCTL0, 16..=20);
    pub const QUEUE_ENABLE: Field<u32, u8> = Field::new_bits(RXDCTL0, 25..=25);
}

pub const TXDCTL0: Register<u32> = Register::new(0xE028).with_le();

pub mod txdctl0 {
    use super::*;
    pub const PTHRESH: Field<u32, u8> = Field::new_bits(TXDCTL0, 0..=4);
    pub const HTHRESH: Field<u32, u8> = Field::new_bits(TXDCTL0, 8..=12);
    pub const WTHRESH: Field<u32, u8> = Field::new_bits(TXDCTL0, 16..=20);
    pub const QUEUE_ENABLE: Field<u32, u8> = Field::new_bits(TXDCTL0, 25..=25);
}

pub const MII_CR_RESTART_AUTO_NEG: u16 = 0x0200;
pub const MII_CR_POWER_DOWN: u16 = 0x0800;
pub const MII_CR_AUTO_NEG_EN: u16 = 0x1000;

pub const DESC_SIZE: usize = 16;

pub const RXD_STATUS_OFFSET: usize = 8;

pub const RXD_LENGTH_OFFSET: usize = 12;

pub const TXD_STATUS_OFFSET: usize = 12;

pub const TXD_CMD_OFFSET: usize = 8;

pub const TXD_OLINFO_OFFSET: usize = 12;

pub const ADVTXD_DTYP_DATA: u32 = 0x0030_0000;
pub const ADVTXD_DCMD_EOP: u32 = 0x0100_0000;
pub const ADVTXD_DCMD_IFCS: u32 = 0x0200_0000;
pub const ADVTXD_DCMD_RS: u32 = 0x0800_0000;
pub const ADVTXD_DCMD_DEXT: u32 = 0x2000_0000;
pub const ADVTXD_PAYLEN_SHIFT: u32 = 14;
pub const TXD_STAT_DD: u32 = 1 << 0;

pub const RXD_STAT_EOP: u32 = 1 << 1;
