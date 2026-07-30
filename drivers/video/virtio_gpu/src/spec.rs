use zinnia::memory::Register;

pub const VIRTIO_GPU_F_VIRGL: u32 = 1 << 0;
pub const VIRTIO_GPU_F_EDID: u32 = 1 << 1;

pub const VIRTIO_F_VERSION_1_LO: u32 = 1;

pub const MAX_SCANOUTS: usize = 16;
pub const FLAG_FENCE: u32 = 1 << 0;

pub const _CAPSET_VIRGL: u32 = 1;
pub const _CAPSET_VIRGL2: u32 = 2;

pub mod config {
    use super::Register;

    pub const _NUM_SCANOUTS: Register<u32> = Register::new(0x08).with_le();
    pub const NUM_CAPSETS: Register<u32> = Register::new(0x0C).with_le();
}

pub mod cmd {
    pub const GET_DISPLAY_INFO: u32 = 0x0100;
    pub const RESOURCE_CREATE_2D: u32 = 0x0101;
    pub const RESOURCE_UNREF: u32 = 0x0102;
    pub const SET_SCANOUT: u32 = 0x0103;
    pub const RESOURCE_FLUSH: u32 = 0x0104;
    pub const TRANSFER_TO_HOST_2D: u32 = 0x0105;
    pub const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
    pub const RESOURCE_DETACH_BACKING: u32 = 0x0107;
    pub const GET_CAPSET_INFO: u32 = 0x0108;
    pub const GET_CAPSET: u32 = 0x0109;

    pub const CTX_CREATE: u32 = 0x0200;
    pub const CTX_DESTROY: u32 = 0x0201;
    pub const CTX_ATTACH_RESOURCE: u32 = 0x0202;
    pub const CTX_DETACH_RESOURCE: u32 = 0x0203;
    pub const RESOURCE_CREATE_3D: u32 = 0x0204;
    pub const TRANSFER_TO_HOST_3D: u32 = 0x0205;
    pub const TRANSFER_FROM_HOST_3D: u32 = 0x0206;
    pub const SUBMIT_3D: u32 = 0x0207;

    pub const UPDATE_CURSOR: u32 = 0x0300;
    pub const MOVE_CURSOR: u32 = 0x0301;
}

pub mod resp {
    pub const OK_NODATA: u32 = 0x1100;
    pub const OK_DISPLAY_INFO: u32 = 0x1101;
    pub const OK_CAPSET_INFO: u32 = 0x1102;
    pub const OK_CAPSET: u32 = 0x1103;

    pub const ERR_UNSPEC: u32 = 0x1200;
    pub const ERR_OUT_OF_MEMORY: u32 = 0x1201;
    pub const ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
    pub const ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
    pub const ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
    pub const ERR_INVALID_PARAMETER: u32 = 0x1205;
}

pub mod format {
    pub const _B8G8R8A8_UNORM: u32 = 1;
    pub const B8G8R8X8_UNORM: u32 = 2;
}

pub mod ctrl_hdr {
    use super::Register;

    pub const SIZE: usize = 24;
    pub const TYPE: Register<u32> = Register::new(0x00).with_le();
    pub const FLAGS: Register<u32> = Register::new(0x04).with_le();
    pub const FENCE_ID: Register<u64> = Register::new(0x08).with_le();
    pub const CTX_ID: Register<u32> = Register::new(0x10).with_le();
    pub const _RING_IDX: Register<u8> = Register::new(0x14);
}

pub mod rect {
    use super::Register;

    pub const SIZE: usize = 16;
    pub const X: Register<u32> = Register::new(0x00).with_le();
    pub const Y: Register<u32> = Register::new(0x04).with_le();
    pub const WIDTH: Register<u32> = Register::new(0x08).with_le();
    pub const HEIGHT: Register<u32> = Register::new(0x0C).with_le();
}

pub mod box3d {
    use super::Register;

    pub const SIZE: usize = 24;
    pub const X: Register<u32> = Register::new(0x00).with_le();
    pub const Y: Register<u32> = Register::new(0x04).with_le();
    pub const Z: Register<u32> = Register::new(0x08).with_le();
    pub const W: Register<u32> = Register::new(0x0C).with_le();
    pub const H: Register<u32> = Register::new(0x10).with_le();
    pub const D: Register<u32> = Register::new(0x14).with_le();
}

pub mod display_one {
    use super::Register;

    pub const SIZE: usize = 24;
    pub const RECT: usize = 0x00;
    pub const ENABLED: Register<u32> = Register::new(0x10).with_le();
}

pub mod resp_display_info {
    pub const SIZE: usize = super::ctrl_hdr::SIZE + super::MAX_SCANOUTS * super::display_one::SIZE;
    pub const PMODES: usize = super::ctrl_hdr::SIZE;
}

pub mod resource_create_2d {
    use super::Register;

    pub const SIZE: usize = 40;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x18).with_le();
    pub const FORMAT: Register<u32> = Register::new(0x1C).with_le();
    pub const WIDTH: Register<u32> = Register::new(0x20).with_le();
    pub const HEIGHT: Register<u32> = Register::new(0x24).with_le();
}

pub mod resource_unref {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x18).with_le();
}

pub mod set_scanout {
    use super::Register;

    pub const SIZE: usize = 48;
    pub const RECT: usize = 0x18;
    pub const SCANOUT_ID: Register<u32> = Register::new(0x28).with_le();
    pub const RESOURCE_ID: Register<u32> = Register::new(0x2C).with_le();
}

pub mod resource_flush {
    use super::Register;

    pub const SIZE: usize = 48;
    pub const RECT: usize = 0x18;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x28).with_le();
}

pub mod transfer_host_2d {
    use super::Register;

    pub const SIZE: usize = 56;
    pub const RECT: usize = 0x18;
    pub const OFFSET: Register<u64> = Register::new(0x28).with_le();
    pub const RESOURCE_ID: Register<u32> = Register::new(0x30).with_le();
}

pub mod attach_backing {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x18).with_le();
    pub const NR_ENTRIES: Register<u32> = Register::new(0x1C).with_le();
}

pub mod detach_backing {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x18).with_le();
}

pub mod mem_entry {
    use super::Register;

    pub const SIZE: usize = 16;
    pub const ADDR: Register<u64> = Register::new(0x00).with_le();
    pub const LENGTH: Register<u32> = Register::new(0x08).with_le();
}

pub mod get_capset_info {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const CAPSET_INDEX: Register<u32> = Register::new(0x18).with_le();
}

pub mod resp_capset_info {
    use super::Register;

    pub const SIZE: usize = 40;
    pub const CAPSET_ID: Register<u32> = Register::new(0x18).with_le();
    pub const CAPSET_MAX_VERSION: Register<u32> = Register::new(0x1C).with_le();
    pub const CAPSET_MAX_SIZE: Register<u32> = Register::new(0x20).with_le();
}

pub mod get_capset {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const CAPSET_ID: Register<u32> = Register::new(0x18).with_le();
    pub const CAPSET_VERSION: Register<u32> = Register::new(0x1C).with_le();
}

pub mod resp_capset {
    pub const DATA: usize = super::ctrl_hdr::SIZE;
}

pub mod ctx_create {
    use super::Register;

    pub const SIZE: usize = 96;
    pub const NLEN: Register<u32> = Register::new(0x18).with_le();
    pub const CONTEXT_INIT: Register<u32> = Register::new(0x1C).with_le();
    pub const DEBUG_NAME: usize = 0x20;
    pub const DEBUG_NAME_LEN: usize = 64;
}

pub mod ctx_destroy {
    pub const SIZE: usize = super::ctrl_hdr::SIZE;
}

pub mod ctx_resource {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x18).with_le();
}

pub mod resource_create_3d {
    use super::Register;

    pub const SIZE: usize = 72;
    pub const RESOURCE_ID: Register<u32> = Register::new(0x18).with_le();
    pub const TARGET: Register<u32> = Register::new(0x1C).with_le();
    pub const FORMAT: Register<u32> = Register::new(0x20).with_le();
    pub const BIND: Register<u32> = Register::new(0x24).with_le();
    pub const WIDTH: Register<u32> = Register::new(0x28).with_le();
    pub const HEIGHT: Register<u32> = Register::new(0x2C).with_le();
    pub const DEPTH: Register<u32> = Register::new(0x30).with_le();
    pub const ARRAY_SIZE: Register<u32> = Register::new(0x34).with_le();
    pub const LAST_LEVEL: Register<u32> = Register::new(0x38).with_le();
    pub const NR_SAMPLES: Register<u32> = Register::new(0x3C).with_le();
    pub const FLAGS: Register<u32> = Register::new(0x40).with_le();
}

pub mod transfer_host_3d {
    use super::Register;

    pub const SIZE: usize = 72;
    pub const BOX: usize = 0x18;
    pub const OFFSET: Register<u64> = Register::new(0x30).with_le();
    pub const RESOURCE_ID: Register<u32> = Register::new(0x38).with_le();
    pub const LEVEL: Register<u32> = Register::new(0x3C).with_le();
    pub const STRIDE: Register<u32> = Register::new(0x40).with_le();
    pub const LAYER_STRIDE: Register<u32> = Register::new(0x44).with_le();
}

pub mod submit_3d {
    use super::Register;

    pub const SIZE: usize = 32;
    pub const STREAM_SIZE: Register<u32> = Register::new(0x18).with_le();
}

pub mod update_cursor {
    use super::Register;

    pub const SIZE: usize = 56;
    pub const POS_SCANOUT_ID: Register<u32> = Register::new(0x18).with_le();
    pub const POS_X: Register<u32> = Register::new(0x1C).with_le();
    pub const POS_Y: Register<u32> = Register::new(0x20).with_le();
    pub const RESOURCE_ID: Register<u32> = Register::new(0x28).with_le();
    pub const HOT_X: Register<u32> = Register::new(0x2C).with_le();
    pub const HOT_Y: Register<u32> = Register::new(0x30).with_le();
}
