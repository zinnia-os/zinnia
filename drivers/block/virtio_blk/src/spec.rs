use zinnia::memory::Register;

pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1 << 1;
pub const VIRTIO_BLK_F_SEG_MAX: u32 = 1 << 2;
// pub const VIRTIO_BLK_F_GEOMETRY: u32 = 1 << 4;
pub const VIRTIO_BLK_F_RO: u32 = 1 << 5;
pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 1 << 6;
// pub const VIRTIO_BLK_F_FLUSH: u32 = 1 << 9;
// pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 1 << 10;
// pub const VIRTIO_BLK_F_CONFIG_WCE: u32 = 1 << 11;
// pub const VIRTIO_BLK_F_MQ: u32 = 1 << 12;
// pub const VIRTIO_BLK_F_DISCARD: u32 = 1 << 13;
// pub const VIRTIO_BLK_F_WRITE_ZEROES: u32 = 1 << 14;

pub const SUPPORTED_FEATURES: u32 =
    VIRTIO_BLK_F_SIZE_MAX | VIRTIO_BLK_F_SEG_MAX | VIRTIO_BLK_F_RO | VIRTIO_BLK_F_BLK_SIZE;

pub const VIRTIO_F_VERSION_1_LO: u32 = 1;

pub const SECTOR_SIZE: usize = 512;
pub const REQUEST_QUEUE: u16 = 0;

pub mod config {
    use super::Register;

    pub const CAPACITY_LO: Register<u32> = Register::new(0x00).with_le();
    pub const CAPACITY_HI: Register<u32> = Register::new(0x04).with_le();
    pub const SIZE_MAX: Register<u32> = Register::new(0x08).with_le();
    pub const SEG_MAX: Register<u32> = Register::new(0x0C).with_le();
    // pub const GEOMETRY: Register<u32> = Register::new(0x10).with_le();
    pub const BLK_SIZE: Register<u32> = Register::new(0x14).with_le();
    // pub const TOPOLOGY: Register<u32> = Register::new(0x18).with_le();
    // pub const NUM_QUEUES: Register<u16> = Register::new(0x22).with_le();
}

pub mod req {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const HEADER_LEN: usize = 16;
    pub const STATUS_LEN: usize = 1;

    pub const TYPE: Register<u32> = Register::new(0x00).with_le();
    // pub const RESERVED: Register<u32> = Register::new(0x04).with_le();
    pub const SECTOR: Register<u64> = Register::new(0x08).with_le();
    pub const STATUS: Register<u8> = Register::new(0x10);
}

pub mod req_type {
    pub const IN: u32 = 0;
    pub const OUT: u32 = 1;
    // pub const FLUSH: u32 = 4;
    // pub const GET_ID: u32 = 8;
    // pub const DISCARD: u32 = 11;
    // pub const WRITE_ZEROES: u32 = 13;
}

pub mod status {
    pub const OK: u8 = 0;
    // pub const IOERR: u8 = 1;
    pub const UNSUPP: u8 = 2;
    pub const UNSET: u8 = 0xFF;
}
