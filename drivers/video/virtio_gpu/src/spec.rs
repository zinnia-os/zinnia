// VirtIO GPU protocol definitions and constants

// Feature bits
pub const _VIRTIO_GPU_F_VIRGL: u32 = 1 << 0;
pub const VIRTIO_GPU_F_EDID: u32 = 1 << 1;
pub const _VIRTIO_GPU_F_RESOURCE_UUID: u32 = 1 << 2;
pub const _VIRTIO_GPU_F_RESOURCE_BLOB: u32 = 1 << 3;
pub const _VIRTIO_GPU_F_CONTEXT_INIT: u32 = 1 << 4;

pub const VIRTIO_GPU_SUPPORTED_FEATURES: u32 = VIRTIO_GPU_F_EDID;

// Control queue command types
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const _VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const _VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
pub const _VIRTIO_GPU_CMD_GET_CAPSET_INFO: u32 = 0x0108;
pub const _VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0109;
pub const _VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010a;
pub const _VIRTIO_GPU_CMD_RESOURCE_ASSIGN_UUID: u32 = 0x010b;
pub const _VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB: u32 = 0x010c;
pub const _VIRTIO_GPU_CMD_SET_SCANOUT_BLOB: u32 = 0x010d;

// Cursor queue command types
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0301;

// Response types
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
pub const _VIRTIO_GPU_RESP_OK_CAPSET_INFO: u32 = 0x1102;
pub const _VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1103;
pub const _VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
pub const _VIRTIO_GPU_RESP_OK_RESOURCE_UUID: u32 = 0x1105;

// Error responses
pub const _VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
pub const _VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const _VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
pub const _VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const _VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
pub const _VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

// Formats
pub const _VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
pub const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;
pub const _VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM: u32 = 3;
pub const _VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM: u32 = 4;
pub const _VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;
pub const _VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM: u32 = 68;
pub const _VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM: u32 = 121;
pub const _VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM: u32 = 134;

pub const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuCtrlHdr {
    pub type_: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub ring_idx: u8,
    pub padding: [u8; 3],
}

impl VirtioGpuCtrlHdr {
    pub const fn new(type_: u32) -> Self {
        Self {
            type_,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            ring_idx: 0,
            padding: [0; 3],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuGetDisplayInfo {
    pub hdr: VirtioGpuCtrlHdr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuDisplayOne {
    pub r: VirtioGpuRect,
    pub enabled: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuRespDisplayInfo {
    pub hdr: VirtioGpuCtrlHdr,
    pub pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuResourceCreate2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct _VirtioGpuResourceUnref {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuSetScanout {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuResourceFlush {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuTransferToHost2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtioGpuResourceAttachBacking {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct _VirtioGpuResourceDetachBacking {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuUpdateCursor {
    pub hdr: VirtioGpuCtrlHdr,
    pub pos: VirtioGpuCursorPos,
    pub resource_id: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuCursorPos {
    pub scanout_id: u32,
    pub x: u32,
    pub y: u32,
    pub padding: u32,
}
