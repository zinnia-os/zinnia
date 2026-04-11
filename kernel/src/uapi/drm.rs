use super::ioctls::*;
use crate::memory::UserPtr;

const BASE: u8 = b'd';

const fn drm_io(num: u8) -> u32 {
    io(BASE, num)
}

const fn drm_ior<T>(num: u8) -> u32 {
    ior::<T>(BASE, num)
}

const fn drm_iow<T>(num: u8) -> u32 {
    iow::<T>(BASE, num)
}

const fn drm_iowr<T>(num: u8) -> u32 {
    iowr::<T>(BASE, num)
}

pub type drm_context_t = u32;
pub type drm_drawable_t = u32;
pub type drm_magic_t = u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_clip_rect {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_drawable_info {
    pub num_rects: u32,
    pub rects: UserPtr<drm_clip_rect>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_version {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    pub name_len: usize,
    pub name: UserPtr<u8>,
    pub date_len: usize,
    pub date: UserPtr<u8>,
    pub desc_len: usize,
    pub desc: UserPtr<u8>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_unique {
    pub unique_len: usize,
    pub unique: UserPtr<u8>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_list {
    pub count: i32,
    pub version: UserPtr<drm_version>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_block {
    unused: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_auth {
    pub magic: drm_magic_t,
}

pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x2;
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
pub const DRM_CAP_PRIME: u64 = 0x5;
pub const DRM_PRIME_CAP_IMPORT: u64 = 0x1;
pub const DRM_PRIME_CAP_EXPORT: u64 = 0x2;
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x8;
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x9;
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
pub const DRM_CAP_PAGE_FLIP_TARGET: u64 = 0x11;
pub const DRM_CAP_CRTC_IN_VBLANK_EVENT: u64 = 0x12;
pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;
pub const DRM_CAP_ATOMIC_ASYNC_PAGE_FLIP: u64 = 0x15;
pub const DRM_CAP_ATOMIC: u64 = 0x3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_get_cap {
    pub capability: u64,
    pub value: u64,
}

pub const DRM_CLIENT_CAP_STEREO_3D: u64 = 1;
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
pub const DRM_CLIENT_CAP_ASPECT_RATIO: u64 = 4;
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;
pub const DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT: u64 = 6;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_set_client_cap {
    pub capability: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_tex_region {
    pub next: u8,
    pub prev: u8,
    pub in_use: u8,
    pub padding: u8,
    pub age: u32,
}

pub const DRM_ADD_COMMAND: u32 = 0;
pub const DRM_RM_COMMAND: u32 = 1;
pub const DRM_INST_HANDLER: u32 = 2;
pub const DRM_UNINST_HANDLER: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_control {
    pub func: u32,
    pub irq: i32,
}

pub type drm_map_type = u32;
pub const DRM_FRAME_BUFFER: drm_map_type = 0;
pub const DRM_REGISTERS: drm_map_type = 1;
pub const DRM_SHM: drm_map_type = 2;
pub const DRM_AGP: drm_map_type = 3;
pub const DRM_SCATTER_GATHER: drm_map_type = 4;
pub const DRM_CONSISTENT: drm_map_type = 5;

pub type drm_map_flags = u32;
pub const DRM_RESTRICTED: drm_map_flags = 0x01;
pub const DRM_READ_ONLY: drm_map_flags = 0x02;
pub const DRM_LOCKED: drm_map_flags = 0x04;
pub const DRM_KERNEL: drm_map_flags = 0x08;
pub const DRM_WRITE_COMBINING: drm_map_flags = 0x10;
pub const DRM_CONTAINS_LOCK: drm_map_flags = 0x20;
pub const DRM_REMOVABLE: drm_map_flags = 0x40;
pub const DRM_DRIVER: drm_map_flags = 0x80;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_ctx_priv_map {
    pub ctx_id: u32,
    pub handle: UserPtr<()>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_map {
    pub offset: usize,
    pub size: usize,
    pub typ: drm_map_type,
    pub flags: drm_map_flags,
    pub handle: UserPtr<()>,
    pub mtrr: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_client {
    pub idx: i32,
    pub auth: i32,
    pub pid: usize,
    pub uid: usize,
    pub magic: usize,
    pub iocs: usize,
}

pub type drm_stat_type = u32;
pub const DRM_STAT_LOCK: drm_stat_type = 0;
pub const DRM_STAT_OPENS: drm_stat_type = 1;
pub const DRM_STAT_CLOSES: drm_stat_type = 2;
pub const DRM_STAT_IOCTLS: drm_stat_type = 3;
pub const DRM_STAT_LOCKS: drm_stat_type = 4;
pub const DRM_STAT_UNLOCKS: drm_stat_type = 5;
pub const DRM_STAT_VALUE: drm_stat_type = 6;
pub const DRM_STAT_BYTE: drm_stat_type = 7;
pub const DRM_STAT_COUNT: drm_stat_type = 8;
pub const DRM_STAT_IRQ: drm_stat_type = 9;
pub const DRM_STAT_PRIMARY: drm_stat_type = 10;
pub const DRM_STAT_SECONDARY: drm_stat_type = 11;
pub const DRM_STAT_DMA: drm_stat_type = 12;
pub const DRM_STAT_SPECIAL: drm_stat_type = 13;
pub const DRM_STAT_MISSED: drm_stat_type = 14;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_stats_data {
    pub value: usize,
    pub typ: drm_stat_type,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_stats {
    pub count: usize,
    pub data: [drm_stats_data; 15],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_draw {
    pub handle: drm_drawable_t,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_set_version {
    pub drm_di_major: i32,
    pub drm_di_minor: i32,
    pub drm_dd_major: i32,
    pub drm_dd_minor: i32,
}

pub type drm_ctx_flags = u32;
pub const DRM_CONTEXT_PRESERVED: drm_ctx_flags = 0x01;
pub const DRM_CONTEXT_2DONLY: drm_ctx_flags = 0x02;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_ctx {
    pub handle: drm_context_t,
    pub flags: drm_ctx_flags,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_ctx_res {
    pub count: i32,
    pub contexts: UserPtr<drm_ctx>,
}

pub const DRM_DISPLAY_MODE_LEN: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct drm_mode_modeinfo {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    pub vrefresh: u32,
    pub flags: u32,
    pub typ: u32,
    pub name: [u8; DRM_DISPLAY_MODE_LEN],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct drm_mode_crtc {
    pub set_connectors_ptr: u64,
    pub count_connectors: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub x: u32,
    pub y: u32,
    pub gamma_size: u32,
    pub mode_valid: u32,
    pub mode: drm_mode_modeinfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_get_encoder {
    pub encoder_id: u32,
    pub encoder_type: u32,
    pub crtc_id: u32,
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

#[derive(Clone, Copy)]
pub enum drm_mode_connector_type {
    Unknown = 0,
    VGA = 1,
    DVII = 2,
    DVID = 3,
    DVIA = 4,
    Composite = 5,
    SVIDEO = 6,
    LVDS = 7,
    Component = 8,
    DIN9Pin = 9,
    DisplayPort = 10,
    HDMIA = 11,
    HDMIB = 12,
    TV = 13,
    eDP = 14,
    Virtual = 15,
    DSI = 16,
    DPI = 17,
    Writeback = 18,
    SPI = 19,
    USB = 20,
}

#[derive(Clone, Copy)]
pub enum drm_mode_connector_state {
    Connected = 1,
    Disconnected = 2,
    Unknown = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct drm_mode_get_connector {
    pub encoders_ptr: u64,
    pub modes_ptr: u64,
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub count_modes: u32,
    pub count_props: u32,
    pub count_encoders: u32,
    pub encoder_id: u32,
    pub connector_id: u32,
    pub connector_type: u32,
    pub connector_type_id: u32,
    pub connection: u32,
    pub mm_width: u32,
    pub mm_height: u32,
    pub subpixel: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_fb_cmd {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub depth: u32,
    pub handle: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct drm_mode_card_res {
    pub fb_id_ptr: u64,
    pub crtc_id_ptr: u64,
    pub connector_id_ptr: u64,
    pub encoder_id_ptr: u64,
    pub count_fbs: u32,
    pub count_crtcs: u32,
    pub count_connectors: u32,
    pub count_encoders: u32,
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_prime_handle {
    pub handle: u32,
    pub flags: u32,
    pub fd: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_crtc_lut {
    pub crtc_id: u32,
    pub gamma_size: u32,
    pub red: u64,
    pub green: u64,
    pub blue: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_list_lessees {
    pub count_lessees: u32,
    pub pad: u32,
    pub lessees_ptr: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_cursor {
    pub flags: u32,
    pub crtc_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub handle: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_cursor2 {
    pub flags: u32,
    pub crtc_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub handle: u32,
    pub hot_x: i32,
    pub hot_y: i32,
}

pub const DRM_PROP_NAME_LEN: usize = 32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_get_property {
    pub values_ptr: u64,
    pub enum_blob_ptr: u64,
    pub prop_id: u32,
    pub flags: u32,
    pub name: [u8; DRM_PROP_NAME_LEN],
    pub count_values: u32,
    pub count_enum_blobs: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_connector_set_property {
    pub value: u64,
    pub prop_id: u32,
    pub connector_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_get_plane {
    pub plane_id: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub possible_crtcs: u32,
    pub gamma_size: u32,
    pub count_format_types: u32,
    pub format_type_ptr: u64,
}

pub const DRM_PLANE_TYPE_OVERLAY: u32 = 0;
pub const DRM_PLANE_TYPE_PRIMARY: u32 = 1;
pub const DRM_PLANE_TYPE_CURSOR: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_get_plane_res {
    pub plane_id_ptr: u64,
    pub count_planes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_create_dumb {
    pub height: u32,
    pub width: u32,
    pub bpp: u32,
    pub flags: u32,
    pub handle: u32,
    pub pitch: u32,
    pub size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_get_blob {
    pub blob_id: u32,
    pub length: u32,
    pub data: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_crtc_page_flip {
    pub crtc_id: u32,
    pub fb_id: u32,
    pub flags: u32,
    pub reserved: u32,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_crtc_get_sequence {
    pub crtc_id: u32,
    pub active: u32,
    pub sequence: u64,
    pub sequence_ns: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_crtc_queue_sequence {
    pub crtc_id: u32,
    pub flags: u32,
    pub sequence: u64,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_fb_dirty_cmd {
    pub fb_id: u32,
    pub flags: u32,
    pub color: u32,
    pub num_clips: u32,
    pub clips_ptr: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_map_dumb {
    pub handle: u32,
    pub pad: u32,
    pub offset: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_destroy_dumb {
    pub handle: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_fb_cmd2 {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub flags: u32,
    pub handles: [u32; 4],
    pub pitches: [u32; 4],
    pub offsets: [u32; 4],
    pub modifier: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_obj_get_properties {
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub count_props: u32,
    pub obj_id: u32,
    pub obj_type: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_atomic {
    pub flags: u32,
    pub count_objs: u32,
    pub objs_ptr: u64,
    pub count_props_ptr: u64,
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub reserved: u64,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_create_blob {
    pub data: u64,
    pub length: u32,
    pub blob_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_destroy_blob {
    pub blob_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct drm_mode_property_enum {
    pub value: u64,
    pub name: [u8; 32],
}

pub const DRM_MODE_OBJECT_CRTC: u32 = 0xcccccccc;
pub const DRM_MODE_OBJECT_CONNECTOR: u32 = 0xc0c0c0c0;
pub const DRM_MODE_OBJECT_ENCODER: u32 = 0xe0e0e0e0;
pub const DRM_MODE_OBJECT_MODE: u32 = 0xdededede;
pub const DRM_MODE_OBJECT_PROPERTY: u32 = 0xb0b0b0b0;
pub const DRM_MODE_OBJECT_FB: u32 = 0xfbfbfbfb;
pub const DRM_MODE_OBJECT_BLOB: u32 = 0xbbbbbbbb;
pub const DRM_MODE_OBJECT_PLANE: u32 = 0xeeeeeeee;
pub const DRM_MODE_OBJECT_ANY: u32 = 0;

pub const DRM_MODE_FLAG_PHSYNC: u32 = 1 << 0;
pub const DRM_MODE_FLAG_NHSYNC: u32 = 1 << 1;
pub const DRM_MODE_FLAG_PVSYNC: u32 = 1 << 2;
pub const DRM_MODE_FLAG_NVSYNC: u32 = 1 << 3;
pub const DRM_MODE_FLAG_INTERLACE: u32 = 1 << 4;
pub const DRM_MODE_TYPE_DRIVER: u32 = 1 << 6;

pub const DRM_IOCTL_VERSION: u32 = drm_iowr::<drm_version>(0x00);
pub const DRM_IOCTL_GET_UNIQUE: u32 = drm_iowr::<drm_unique>(0x01);
// pub const DRM_IOCTL_GET_MAGIC: u32 = drm_ior::<drm_auth>(0x02);
// pub const DRM_IOCTL_IRQ_BUSID: u32 = drm_iowr::<drm_irq_busid>(0x03);
pub const DRM_IOCTL_GET_MAP: u32 = drm_iowr::<drm_map>(0x04);
pub const DRM_IOCTL_GET_CLIENT: u32 = drm_iowr::<drm_client>(0x05);
pub const DRM_IOCTL_GET_STATS: u32 = drm_ior::<drm_stats>(0x06);
pub const DRM_IOCTL_SET_VERSION: u32 = drm_iowr::<drm_set_version>(0x07);
// pub const DRM_IOCTL_MODESET_CTL: u32 = drm_iow::<drm_modeset_ctl>(0x08);
// pub const DRM_IOCTL_GEM_CLOSE: u32 = drm_iow ::<drm_gem_close>(0x09);
// pub const DRM_IOCTL_GEM_FLINK: u32 = drm_iowr::<drm_gem_flink>(0x0a);
// pub const DRM_IOCTL_GEM_OPEN: u32 = drm_iowr::<drm_gem_open>(0x0b);
pub const DRM_IOCTL_GET_CAP: u32 = drm_iowr::<drm_get_cap>(0x0c);
pub const DRM_IOCTL_SET_CLIENT_CAP: u32 = drm_iow::<drm_set_client_cap>(0x0d);
pub const DRM_IOCTL_SET_UNIQUE: u32 = drm_iow::<drm_unique>(0x10);
// pub const DRM_IOCTL_AUTH_MAGIC: u32 = drm_iow::<drm_auth>(0x11);
// pub const DRM_IOCTL_BLOCK: u32 = drm_iowr::<drm_block>(0x12);
// pub const DRM_IOCTL_UNBLOCK: u32 = drm_iowr::<drm_block>(0x13);
pub const DRM_IOCTL_CONTROL: u32 = drm_iow::<drm_control>(0x14);
pub const DRM_IOCTL_ADD_MAP: u32 = drm_iowr::<drm_map>(0x15);
// pub const DRM_IOCTL_ADD_BUFS: u32 = drm_iowr::<drm_buf_desc>(0x16);
// pub const DRM_IOCTL_MARK_BUFS: u32 = drm_iow::<drm_buf_desc>(0x17);
// pub const DRM_IOCTL_INFO_BUFS: u32 = drm_iowr::<drm_buf_info>(0x18);
// pub const DRM_IOCTL_MAP_BUFS: u32 = drm_iowr::<drm_buf_map>(0x19);
// pub const DRM_IOCTL_FREE_BUFS: u32 = drm_iow::<drm_buf_free>(0x1a);
pub const DRM_IOCTL_RM_MAP: u32 = drm_iow::<drm_map>(0x1b);
// pub const DRM_IOCTL_SET_SAREA_CTX: u32 = drm_iow::<drm_ctx_priv_map>(0x1c);
// pub const DRM_IOCTL_GET_SAREA_CTX: u32 = drm_iowr::<drm_ctx_priv_map>(0x1d);
pub const DRM_IOCTL_SET_MASTER: u32 = drm_io(0x1e);
pub const DRM_IOCTL_DROP_MASTER: u32 = drm_io(0x1f);
pub const DRM_IOCTL_ADD_CTX: u32 = drm_iowr::<drm_ctx>(0x20);
pub const DRM_IOCTL_RM_CTX: u32 = drm_iowr::<drm_ctx>(0x21);
pub const DRM_IOCTL_MOD_CTX: u32 = drm_iow::<drm_ctx>(0x22);
pub const DRM_IOCTL_GET_CTX: u32 = drm_iowr::<drm_ctx>(0x23);
pub const DRM_IOCTL_SWITCH_CTX: u32 = drm_iow::<drm_ctx>(0x24);
pub const DRM_IOCTL_NEW_CTX: u32 = drm_iow::<drm_ctx>(0x25);
pub const DRM_IOCTL_RES_CTX: u32 = drm_iowr::<drm_ctx_res>(0x26);
pub const DRM_IOCTL_ADD_DRAW: u32 = drm_iowr::<drm_draw>(0x27);
pub const DRM_IOCTL_RM_DRAW: u32 = drm_iowr::<drm_draw>(0x28);
// pub const DRM_IOCTL_DMA: u32 = drm_iowr::<drm_dma>(0x29);
// pub const DRM_IOCTL_LOCK: u32 = drm_iow::<drm_lock>(0x2a);
// pub const DRM_IOCTL_UNLOCK: u32 = drm_iow::<drm_lock>(0x2b);
// pub const DRM_IOCTL_FINISH: u32 = drm_iow::<drm_lock>(0x2c);
pub const DRM_IOCTL_PRIME_HANDLE_TO_FD: u32 = drm_iowr::<drm_prime_handle>(0x2d);
pub const DRM_IOCTL_PRIME_FD_TO_HANDLE: u32 = drm_iowr::<drm_prime_handle>(0x2e);
// pub const DRM_IOCTL_AGP_ENABLE: u32 = drm_iow::<drm_agp_mode>(0x32);
// pub const DRM_IOCTL_AGP_INFO: u32 = drm_ior::<drm_agp_info>(0x33);
// pub const DRM_IOCTL_AGP_ALLOC: u32 = drm_iowr::<drm_agp_buffer>(0x34);
// pub const DRM_IOCTL_AGP_FREE: u32 = drm_iow::<drm_agp_buffer>(0x35);
// pub const DRM_IOCTL_AGP_BIND: u32 = drm_iow::<drm_agp_binding>(0x36);
// pub const DRM_IOCTL_AGP_UNBIND: u32 = drm_iow::<drm_agp_binding>(0x37);
// pub const DRM_IOCTL_SG_ALLOC: u32 = drm_iowr::<drm_scatter_gather>(0x38);
// pub const DRM_IOCTL_SG_FREE: u32 = drm_iow::<drm_scatter_gather>(0x39);
// pub const DRM_IOCTL_WAIT_VBLANK: u32 = drm_iowr::<drm_wait_vblank>(0x3a);
pub const DRM_IOCTL_CRTC_GET_SEQUENCE: u32 = drm_iowr::<drm_crtc_get_sequence>(0x3b);
pub const DRM_IOCTL_CRTC_QUEUE_SEQUENCE: u32 = drm_iowr::<drm_crtc_queue_sequence>(0x3c);
// pub const DRM_IOCTL_UPDATE_DRAW: u32 = drm_iow::<drm_update_draw>(0x3f);
pub const DRM_IOCTL_MODE_GETRESOURCES: u32 = drm_iowr::<drm_mode_card_res>(0xA0);
pub const DRM_IOCTL_MODE_GETCRTC: u32 = drm_iowr::<drm_mode_crtc>(0xA1);
pub const DRM_IOCTL_MODE_SETCRTC: u32 = drm_iowr::<drm_mode_crtc>(0xA2);
pub const DRM_IOCTL_MODE_CURSOR: u32 = drm_iowr::<drm_mode_cursor>(0xA3);
// pub const DRM_IOCTL_MODE_GETGAMMA: u32 = drm_iowr::<drm_mode_crtc_lut>(0xA4);
pub const DRM_IOCTL_MODE_SETGAMMA: u32 = drm_iowr::<drm_mode_crtc_lut>(0xA5);
pub const DRM_IOCTL_MODE_GETENCODER: u32 = drm_iowr::<drm_mode_get_encoder>(0xA6);
pub const DRM_IOCTL_MODE_GETCONNECTOR: u32 = drm_iowr::<drm_mode_get_connector>(0xA7);
pub const DRM_IOCTL_MODE_GETPROPERTY: u32 = drm_iowr::<drm_mode_get_property>(0xAA);
pub const DRM_IOCTL_MODE_SETPROPERTY: u32 = drm_iowr::<drm_mode_connector_set_property>(0xAB);
pub const DRM_IOCTL_MODE_GETPROPBLOB: u32 = drm_iowr::<drm_mode_get_blob>(0xAC);
pub const DRM_IOCTL_MODE_GETFB: u32 = drm_iowr::<drm_mode_fb_cmd>(0xAD);
pub const DRM_IOCTL_MODE_ADDFB: u32 = drm_iowr::<drm_mode_fb_cmd>(0xAE);
pub const DRM_IOCTL_MODE_RMFB: u32 = drm_iowr::<u32>(0xAF);
pub const DRM_IOCTL_MODE_PAGE_FLIP: u32 = drm_iowr::<drm_mode_crtc_page_flip>(0xB0);
pub const DRM_IOCTL_MODE_DIRTYFB: u32 = drm_iowr::<drm_mode_fb_dirty_cmd>(0xB1);
pub const DRM_IOCTL_MODE_CREATE_DUMB: u32 = drm_iowr::<drm_mode_create_dumb>(0xB2);
pub const DRM_IOCTL_MODE_MAP_DUMB: u32 = drm_iowr::<drm_mode_map_dumb>(0xB3);
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u32 = drm_iowr::<drm_mode_destroy_dumb>(0xB4);
pub const DRM_IOCTL_MODE_GETPLANERESOURCES: u32 = drm_iowr::<drm_mode_get_plane_res>(0xB5);
pub const DRM_IOCTL_MODE_GETPLANE: u32 = drm_iowr::<drm_mode_get_plane>(0xB6);
// pub const DRM_IOCTL_MODE_SETPLANE: u32 = drm_iowr::<drm_mode_set_plane>(0xB7);
pub const DRM_IOCTL_MODE_ADDFB2: u32 = drm_iowr::<drm_mode_fb_cmd2>(0xB8);
pub const DRM_IOCTL_MODE_OBJ_GETPROPERTIES: u32 = drm_iowr::<drm_mode_obj_get_properties>(0xB9);
// pub const DRM_IOCTL_MODE_OBJ_SETPROPERTY: u32 = drm_iowr::<drm_mode_obj_set_property>(0xBA);
pub const DRM_IOCTL_MODE_CURSOR2: u32 = drm_iowr::<drm_mode_cursor2>(0xBB);
pub const DRM_IOCTL_MODE_ATOMIC: u32 = drm_iowr::<drm_mode_atomic>(0xBC);
pub const DRM_IOCTL_MODE_CREATEPROPBLOB: u32 = drm_iowr::<drm_mode_create_blob>(0xBD);
pub const DRM_IOCTL_MODE_DESTROYPROPBLOB: u32 = drm_iowr::<drm_mode_destroy_blob>(0xBE);
// pub const DRM_IOCTL_SYNCOBJ_CREATE: u32 = drm_iowr::<drm_syncobj_create>(0xBF);
// pub const DRM_IOCTL_SYNCOBJ_DESTROY: u32 = drm_iowr::<drm_syncobj_destroy>(0xC0);
// pub const DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD: u32 = drm_iowr::<drm_syncobj_handle>(0xC1);
// pub const DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE: u32 = drm_iowr::<drm_syncobj_handle>(0xC2);
// pub const DRM_IOCTL_SYNCOBJ_WAIT: u32 = drm_iowr::<drm_syncobj_wait>(0xC3);
// pub const DRM_IOCTL_SYNCOBJ_RESET: u32 = drm_iowr::<drm_syncobj_array>(0xC4);
// pub const DRM_IOCTL_SYNCOBJ_SIGNAL: u32 = drm_iowr::<drm_syncobj_array>(0xC5);
// pub const DRM_IOCTL_MODE_CREATE_LEASE: u32 = drm_iowr::<drm_mode_create_lease>(0xC6);
pub const DRM_IOCTL_MODE_LIST_LESSEES: u32 = drm_iowr::<drm_mode_list_lessees>(0xC7);
// pub const DRM_IOCTL_MODE_GET_LEASE: u32 = drm_iowr::<drm_mode_get_lease>(0xC8);
// pub const DRM_IOCTL_MODE_REVOKE_LEASE: u32 = drm_iowr::<drm_mode_revoke_lease>(0xC9);
// pub const DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT: u32 = drm_iowr::<drm_syncobj_timeline_wait>(0xCA);
// pub const DRM_IOCTL_SYNCOBJ_QUERY: u32 = drm_iowr::<drm_syncobj_timeline_array>(0xCB);
// pub const DRM_IOCTL_SYNCOBJ_TRANSFER: u32 = drm_iowr::<drm_syncobj_transfer>(0xCC);
// pub const DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL: u32 = drm_iowr::<drm_syncobj_timeline_array>(0xCD);
pub const DRM_IOCTL_MODE_GETFB2: u32 = drm_iowr::<drm_mode_fb_cmd2>(0xCE);
// pub const DRM_IOCTL_SYNCOBJ_EVENTFD: u32 = drm_iowr::<drm_syncobj_eventfd>(0xCF);
// pub const DRM_IOCTL_MODE_CLOSEFB: u32 = drm_iowr::<drm_mode_closefb>(0xD0);
// pub const DRM_IOCTL_SET_CLIENT_NAME: u32 = drm_iowr::<drm_set_client_name>(0xD1);
// pub const DRM_IOCTL_GEM_CHANGE_HANDLE: u32 = drm_iowr::<drm_gem_change_handle>(0xD2);

const fn fourcc_code(a: char, b: char, c: char, d: char) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

pub const DRM_FORMAT_BIG_ENDIAN: u32 = 1 << 31;
pub const DRM_FORMAT_INVALID: u32 = 0;
pub const DRM_FORMAT_C1: u32 = fourcc_code('C', '1', ' ', ' ');
pub const DRM_FORMAT_C2: u32 = fourcc_code('C', '2', ' ', ' ');
pub const DRM_FORMAT_C4: u32 = fourcc_code('C', '4', ' ', ' ');
pub const DRM_FORMAT_C8: u32 = fourcc_code('C', '8', ' ', ' ');
pub const DRM_FORMAT_D1: u32 = fourcc_code('D', '1', ' ', ' ');
pub const DRM_FORMAT_D2: u32 = fourcc_code('D', '2', ' ', ' ');
pub const DRM_FORMAT_D4: u32 = fourcc_code('D', '4', ' ', ' ');
pub const DRM_FORMAT_D8: u32 = fourcc_code('D', '8', ' ', ' ');
pub const DRM_FORMAT_R1: u32 = fourcc_code('R', '1', ' ', ' ');
pub const DRM_FORMAT_R2: u32 = fourcc_code('R', '2', ' ', ' ');
pub const DRM_FORMAT_R4: u32 = fourcc_code('R', '4', ' ', ' ');
pub const DRM_FORMAT_R8: u32 = fourcc_code('R', '8', ' ', ' ');
pub const DRM_FORMAT_R10: u32 = fourcc_code('R', '1', '0', ' ');
pub const DRM_FORMAT_R12: u32 = fourcc_code('R', '1', '2', ' ');
pub const DRM_FORMAT_R16: u32 = fourcc_code('R', '1', '6', ' ');
pub const DRM_FORMAT_RG88: u32 = fourcc_code('R', 'G', '8', '8');
pub const DRM_FORMAT_GR88: u32 = fourcc_code('G', 'R', '8', '8');
pub const DRM_FORMAT_RG1616: u32 = fourcc_code('R', 'G', '3', '2');
pub const DRM_FORMAT_GR1616: u32 = fourcc_code('G', 'R', '3', '2');
pub const DRM_FORMAT_RGB332: u32 = fourcc_code('R', 'G', 'B', '8');
pub const DRM_FORMAT_BGR233: u32 = fourcc_code('B', 'G', 'R', '8');
pub const DRM_FORMAT_XRGB4444: u32 = fourcc_code('X', 'R', '1', '2');
pub const DRM_FORMAT_XBGR4444: u32 = fourcc_code('X', 'B', '1', '2');
pub const DRM_FORMAT_RGBX4444: u32 = fourcc_code('R', 'X', '1', '2');
pub const DRM_FORMAT_BGRX4444: u32 = fourcc_code('B', 'X', '1', '2');
pub const DRM_FORMAT_ARGB4444: u32 = fourcc_code('A', 'R', '1', '2');
pub const DRM_FORMAT_ABGR4444: u32 = fourcc_code('A', 'B', '1', '2');
pub const DRM_FORMAT_RGBA4444: u32 = fourcc_code('R', 'A', '1', '2');
pub const DRM_FORMAT_BGRA4444: u32 = fourcc_code('B', 'A', '1', '2');
pub const DRM_FORMAT_XRGB1555: u32 = fourcc_code('X', 'R', '1', '5');
pub const DRM_FORMAT_XBGR1555: u32 = fourcc_code('X', 'B', '1', '5');
pub const DRM_FORMAT_RGBX5551: u32 = fourcc_code('R', 'X', '1', '5');
pub const DRM_FORMAT_BGRX5551: u32 = fourcc_code('B', 'X', '1', '5');
pub const DRM_FORMAT_ARGB1555: u32 = fourcc_code('A', 'R', '1', '5');
pub const DRM_FORMAT_ABGR1555: u32 = fourcc_code('A', 'B', '1', '5');
pub const DRM_FORMAT_RGBA5551: u32 = fourcc_code('R', 'A', '1', '5');
pub const DRM_FORMAT_BGRA5551: u32 = fourcc_code('B', 'A', '1', '5');
pub const DRM_FORMAT_RGB565: u32 = fourcc_code('R', 'G', '1', '6');
pub const DRM_FORMAT_BGR565: u32 = fourcc_code('B', 'G', '1', '6');
pub const DRM_FORMAT_RGB888: u32 = fourcc_code('R', 'G', '2', '4');
pub const DRM_FORMAT_BGR888: u32 = fourcc_code('B', 'G', '2', '4');
pub const DRM_FORMAT_XRGB8888: u32 = fourcc_code('X', 'R', '2', '4');
pub const DRM_FORMAT_XBGR8888: u32 = fourcc_code('X', 'B', '2', '4');
pub const DRM_FORMAT_RGBX8888: u32 = fourcc_code('R', 'X', '2', '4');
pub const DRM_FORMAT_BGRX8888: u32 = fourcc_code('B', 'X', '2', '4');
pub const DRM_FORMAT_ARGB8888: u32 = fourcc_code('A', 'R', '2', '4');
pub const DRM_FORMAT_ABGR8888: u32 = fourcc_code('A', 'B', '2', '4');
pub const DRM_FORMAT_RGBA8888: u32 = fourcc_code('R', 'A', '2', '4');
pub const DRM_FORMAT_BGRA8888: u32 = fourcc_code('B', 'A', '2', '4');
pub const DRM_FORMAT_XRGB2101010: u32 = fourcc_code('X', 'R', '3', '0');
pub const DRM_FORMAT_XBGR2101010: u32 = fourcc_code('X', 'B', '3', '0');
pub const DRM_FORMAT_RGBX1010102: u32 = fourcc_code('R', 'X', '3', '0');
pub const DRM_FORMAT_BGRX1010102: u32 = fourcc_code('B', 'X', '3', '0');
pub const DRM_FORMAT_ARGB2101010: u32 = fourcc_code('A', 'R', '3', '0');
pub const DRM_FORMAT_ABGR2101010: u32 = fourcc_code('A', 'B', '3', '0');
pub const DRM_FORMAT_RGBA1010102: u32 = fourcc_code('R', 'A', '3', '0');
pub const DRM_FORMAT_BGRA1010102: u32 = fourcc_code('B', 'A', '3', '0');
pub const DRM_FORMAT_XRGB16161616: u32 = fourcc_code('X', 'R', '4', '8');
pub const DRM_FORMAT_XBGR16161616: u32 = fourcc_code('X', 'B', '4', '8');
pub const DRM_FORMAT_ARGB16161616: u32 = fourcc_code('A', 'R', '4', '8');
pub const DRM_FORMAT_ABGR16161616: u32 = fourcc_code('A', 'B', '4', '8');
pub const DRM_FORMAT_XRGB16161616F: u32 = fourcc_code('X', 'R', '4', 'H');
pub const DRM_FORMAT_XBGR16161616F: u32 = fourcc_code('X', 'B', '4', 'H');
pub const DRM_FORMAT_ARGB16161616F: u32 = fourcc_code('A', 'R', '4', 'H');
pub const DRM_FORMAT_ABGR16161616F: u32 = fourcc_code('A', 'B', '4', 'H');
pub const DRM_FORMAT_AXBXGXRX106106106106: u32 = fourcc_code('A', 'B', '1', '0');
pub const DRM_FORMAT_YUYV: u32 = fourcc_code('Y', 'U', 'Y', 'V');
pub const DRM_FORMAT_YVYU: u32 = fourcc_code('Y', 'V', 'Y', 'U');
pub const DRM_FORMAT_UYVY: u32 = fourcc_code('U', 'Y', 'V', 'Y');
pub const DRM_FORMAT_VYUY: u32 = fourcc_code('V', 'Y', 'U', 'Y');
pub const DRM_FORMAT_AYUV: u32 = fourcc_code('A', 'Y', 'U', 'V');
pub const DRM_FORMAT_AVUY8888: u32 = fourcc_code('A', 'V', 'U', 'Y');
pub const DRM_FORMAT_XYUV8888: u32 = fourcc_code('X', 'Y', 'U', 'V');
pub const DRM_FORMAT_XVUY8888: u32 = fourcc_code('X', 'V', 'U', 'Y');
pub const DRM_FORMAT_VUY888: u32 = fourcc_code('V', 'U', '2', '4');
pub const DRM_FORMAT_VUY101010: u32 = fourcc_code('V', 'U', '3', '0');
pub const DRM_FORMAT_Y210: u32 = fourcc_code('Y', '2', '1', '0');
pub const DRM_FORMAT_Y212: u32 = fourcc_code('Y', '2', '1', '2');
pub const DRM_FORMAT_Y216: u32 = fourcc_code('Y', '2', '1', '6');
pub const DRM_FORMAT_Y410: u32 = fourcc_code('Y', '4', '1', '0');
pub const DRM_FORMAT_Y412: u32 = fourcc_code('Y', '4', '1', '2');
pub const DRM_FORMAT_Y416: u32 = fourcc_code('Y', '4', '1', '6');
pub const DRM_FORMAT_XVYU2101010: u32 = fourcc_code('X', 'V', '3', '0');
pub const DRM_FORMAT_XVYU12_16161616: u32 = fourcc_code('X', 'V', '3', '6');
pub const DRM_FORMAT_XVYU16161616: u32 = fourcc_code('X', 'V', '4', '8');
pub const DRM_FORMAT_Y0L0: u32 = fourcc_code('Y', '0', 'L', '0');
pub const DRM_FORMAT_X0L0: u32 = fourcc_code('X', '0', 'L', '0');
pub const DRM_FORMAT_Y0L2: u32 = fourcc_code('Y', '0', 'L', '2');
pub const DRM_FORMAT_X0L2: u32 = fourcc_code('X', '0', 'L', '2');
pub const DRM_FORMAT_YUV420_8BIT: u32 = fourcc_code('Y', 'U', '0', '8');
pub const DRM_FORMAT_YUV420_10BIT: u32 = fourcc_code('Y', 'U', '1', '0');
pub const DRM_FORMAT_XRGB8888_A8: u32 = fourcc_code('X', 'R', 'A', '8');
pub const DRM_FORMAT_XBGR8888_A8: u32 = fourcc_code('X', 'B', 'A', '8');
pub const DRM_FORMAT_RGBX8888_A8: u32 = fourcc_code('R', 'X', 'A', '8');
pub const DRM_FORMAT_BGRX8888_A8: u32 = fourcc_code('B', 'X', 'A', '8');
pub const DRM_FORMAT_RGB888_A8: u32 = fourcc_code('R', '8', 'A', '8');
pub const DRM_FORMAT_BGR888_A8: u32 = fourcc_code('B', '8', 'A', '8');
pub const DRM_FORMAT_RGB565_A8: u32 = fourcc_code('R', '5', 'A', '8');
pub const DRM_FORMAT_BGR565_A8: u32 = fourcc_code('B', '5', 'A', '8');
pub const DRM_FORMAT_NV12: u32 = fourcc_code('N', 'V', '1', '2');
pub const DRM_FORMAT_NV21: u32 = fourcc_code('N', 'V', '2', '1');
pub const DRM_FORMAT_NV16: u32 = fourcc_code('N', 'V', '1', '6');
pub const DRM_FORMAT_NV61: u32 = fourcc_code('N', 'V', '6', '1');
pub const DRM_FORMAT_NV24: u32 = fourcc_code('N', 'V', '2', '4');
pub const DRM_FORMAT_NV42: u32 = fourcc_code('N', 'V', '4', '2');
pub const DRM_FORMAT_NV15: u32 = fourcc_code('N', 'V', '1', '5');
pub const DRM_FORMAT_NV20: u32 = fourcc_code('N', 'V', '2', '0');
pub const DRM_FORMAT_NV30: u32 = fourcc_code('N', 'V', '3', '0');
pub const DRM_FORMAT_P210: u32 = fourcc_code('P', '2', '1', '0');
pub const DRM_FORMAT_P010: u32 = fourcc_code('P', '0', '1', '0');
pub const DRM_FORMAT_P012: u32 = fourcc_code('P', '0', '1', '2');
pub const DRM_FORMAT_P016: u32 = fourcc_code('P', '0', '1', '6');
pub const DRM_FORMAT_P030: u32 = fourcc_code('P', '0', '3', '0');
pub const DRM_FORMAT_Q410: u32 = fourcc_code('Q', '4', '1', '0');
pub const DRM_FORMAT_Q401: u32 = fourcc_code('Q', '4', '0', '1');
pub const DRM_FORMAT_YUV410: u32 = fourcc_code('Y', 'U', 'V', '9');
pub const DRM_FORMAT_YVU410: u32 = fourcc_code('Y', 'V', 'U', '9');
pub const DRM_FORMAT_YUV411: u32 = fourcc_code('Y', 'U', '1', '1');
pub const DRM_FORMAT_YVU411: u32 = fourcc_code('Y', 'V', '1', '1');
pub const DRM_FORMAT_YUV420: u32 = fourcc_code('Y', 'U', '1', '2');
pub const DRM_FORMAT_YVU420: u32 = fourcc_code('Y', 'V', '1', '2');
pub const DRM_FORMAT_YUV422: u32 = fourcc_code('Y', 'U', '1', '6');
pub const DRM_FORMAT_YVU422: u32 = fourcc_code('Y', 'V', '1', '6');
pub const DRM_FORMAT_YUV444: u32 = fourcc_code('Y', 'U', '2', '4');
pub const DRM_FORMAT_YVU444: u32 = fourcc_code('Y', 'V', '2', '4');
