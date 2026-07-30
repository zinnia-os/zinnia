use crate::uapi::drm::drm_driver_iowr;

pub const VIRTGPU_PARAM_3D_FEATURES: u64 = 1;
pub const VIRTGPU_PARAM_CAPSET_QUERY_FIX: u64 = 2;
pub const VIRTGPU_PARAM_RESOURCE_BLOB: u64 = 3;
pub const VIRTGPU_PARAM_HOST_VISIBLE: u64 = 4;
pub const VIRTGPU_PARAM_CROSS_DEVICE: u64 = 5;
pub const VIRTGPU_PARAM_CONTEXT_INIT: u64 = 6;
pub const VIRTGPU_PARAM_SUPPORTED_CAPSET_IDS: u64 = 7;
pub const VIRTGPU_PARAM_EXPLICIT_DEBUG_NAME: u64 = 8;

pub const VIRTGPU_DRM_CAPSET_VIRGL: u32 = 1;
pub const VIRTGPU_DRM_CAPSET_VIRGL2: u32 = 2;

pub const VIRTGPU_EXECBUF_FENCE_FD_IN: u32 = 0x01;
pub const VIRTGPU_EXECBUF_FENCE_FD_OUT: u32 = 0x02;
pub const VIRTGPU_EXECBUF_RING_IDX: u32 = 0x04;

pub const VIRTGPU_WAIT_NOWAIT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_map {
    pub offset: u64,
    pub handle: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_execbuffer {
    pub flags: u32,
    pub size: u32,
    pub command: u64,
    pub bo_handles: u64,
    pub num_bo_handles: u32,
    pub fence_fd: i32,
    pub ring_idx: u32,
    pub syncobj_stride: u32,
    pub num_in_syncobjs: u32,
    pub num_out_syncobjs: u32,
    pub in_syncobjs: u64,
    pub out_syncobjs: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_getparam {
    pub param: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_resource_create {
    pub target: u32,
    pub format: u32,
    pub bind: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: u32,
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u32,
    pub stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_resource_info {
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u32,
    pub blob_mem: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_3d_box {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_3d_transfer_to_host {
    pub bo_handle: u32,
    pub r#box: drm_virtgpu_3d_box,
    pub level: u32,
    pub offset: u32,
    pub stride: u32,
    pub layer_stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_3d_transfer_from_host {
    pub bo_handle: u32,
    pub r#box: drm_virtgpu_3d_box,
    pub level: u32,
    pub offset: u32,
    pub stride: u32,
    pub layer_stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_3d_wait {
    pub handle: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_virtgpu_get_caps {
    pub cap_set_id: u32,
    pub cap_set_ver: u32,
    pub addr: u64,
    pub size: u32,
    pub pad: u32,
}

pub const DRM_VIRTGPU_MAP: u8 = 0x01;
pub const DRM_VIRTGPU_EXECBUFFER: u8 = 0x02;
pub const DRM_VIRTGPU_GETPARAM: u8 = 0x03;
pub const DRM_VIRTGPU_RESOURCE_CREATE: u8 = 0x04;
pub const DRM_VIRTGPU_RESOURCE_INFO: u8 = 0x05;
pub const DRM_VIRTGPU_TRANSFER_FROM_HOST: u8 = 0x06;
pub const DRM_VIRTGPU_TRANSFER_TO_HOST: u8 = 0x07;
pub const DRM_VIRTGPU_WAIT: u8 = 0x08;
pub const DRM_VIRTGPU_GET_CAPS: u8 = 0x09;

pub const DRM_IOCTL_VIRTGPU_MAP: u32 = drm_driver_iowr::<drm_virtgpu_map>(DRM_VIRTGPU_MAP);
pub const DRM_IOCTL_VIRTGPU_EXECBUFFER: u32 =
    drm_driver_iowr::<drm_virtgpu_execbuffer>(DRM_VIRTGPU_EXECBUFFER);
pub const DRM_IOCTL_VIRTGPU_GETPARAM: u32 =
    drm_driver_iowr::<drm_virtgpu_getparam>(DRM_VIRTGPU_GETPARAM);
pub const DRM_IOCTL_VIRTGPU_RESOURCE_CREATE: u32 =
    drm_driver_iowr::<drm_virtgpu_resource_create>(DRM_VIRTGPU_RESOURCE_CREATE);
pub const DRM_IOCTL_VIRTGPU_RESOURCE_INFO: u32 =
    drm_driver_iowr::<drm_virtgpu_resource_info>(DRM_VIRTGPU_RESOURCE_INFO);
pub const DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST: u32 =
    drm_driver_iowr::<drm_virtgpu_3d_transfer_from_host>(DRM_VIRTGPU_TRANSFER_FROM_HOST);
pub const DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST: u32 =
    drm_driver_iowr::<drm_virtgpu_3d_transfer_to_host>(DRM_VIRTGPU_TRANSFER_TO_HOST);
pub const DRM_IOCTL_VIRTGPU_WAIT: u32 = drm_driver_iowr::<drm_virtgpu_3d_wait>(DRM_VIRTGPU_WAIT);
pub const DRM_IOCTL_VIRTGPU_GET_CAPS: u32 =
    drm_driver_iowr::<drm_virtgpu_get_caps>(DRM_VIRTGPU_GET_CAPS);
