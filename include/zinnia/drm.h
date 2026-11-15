/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_DRM_H
#define __ZINNIA_DRM_H

#include <zinnia/fcntl.h>
#include <zinnia/ioctls.h>
#include <zinnia/types.h>

#define DRM_BASE 'd'

#define DRM_COMMAND_BASE 0x40
#define DRM_COMMAND_END  0xA0

#define DRM_IO(num)                __IO(DRM_BASE, (num))
#define DRM_IOR(num, type)         __IOR(DRM_BASE, (num), type)
#define DRM_IOW(num, type)         __IOW(DRM_BASE, (num), type)
#define DRM_IOWR(num, type)        __IOWR(DRM_BASE, (num), type)
#define DRM_DRIVER_IOWR(num, type) __IOWR(DRM_BASE, DRM_COMMAND_BASE + (num), type)

typedef __zn_u32 drm_context_t;
typedef __zn_u32 drm_drawable_t;
typedef __zn_u32 drm_magic_t;

struct drm_clip_rect {
    __zn_u16 x1;
    __zn_u16 y1;
    __zn_u16 x2;
    __zn_u16 y2;
};

struct drm_drawable_info {
    __zn_u32 num_rects;
    __zn_uptr rects;
};

struct drm_version {
    __zn_i32 version_major;
    __zn_i32 version_minor;
    __zn_i32 version_patchlevel;
    __zn_size name_len;
    __zn_uptr name;
    __zn_size date_len;
    __zn_uptr date;
    __zn_size desc_len;
    __zn_uptr desc;
};

struct drm_unique {
    __zn_size unique_len;
    __zn_uptr unique;
};

struct drm_list {
    __zn_i32 count;
    __zn_uptr version;
};

struct drm_block {
    __zn_u32 unused;
};

struct drm_auth {
    drm_magic_t magic;
};

#define DRM_CAP_DUMB_BUFFER            0x1
#define DRM_CAP_VBLANK_HIGH_CRTC       0x2
#define DRM_CAP_DUMB_PREFERRED_DEPTH   0x3
#define DRM_CAP_DUMB_PREFER_SHADOW     0x4
#define DRM_CAP_PRIME                  0x5
#define DRM_PRIME_CAP_IMPORT           0x1
#define DRM_PRIME_CAP_EXPORT           0x2
#define DRM_CAP_TIMESTAMP_MONOTONIC    0x6
#define DRM_CAP_ASYNC_PAGE_FLIP        0x7
#define DRM_CAP_CURSOR_WIDTH           0x8
#define DRM_CAP_CURSOR_HEIGHT          0x9
#define DRM_CAP_ADDFB2_MODIFIERS       0x10
#define DRM_CAP_PAGE_FLIP_TARGET       0x11
#define DRM_CAP_CRTC_IN_VBLANK_EVENT   0x12
#define DRM_CAP_SYNCOBJ                0x13
#define DRM_CAP_SYNCOBJ_TIMELINE       0x14
#define DRM_CAP_ATOMIC_ASYNC_PAGE_FLIP 0x15

struct drm_get_cap {
    __zn_u64 capability;
    __zn_u64 value;
};

#define DRM_CLIENT_CAP_STEREO_3D            1
#define DRM_CLIENT_CAP_UNIVERSAL_PLANES     2
#define DRM_CLIENT_CAP_ATOMIC               3
#define DRM_CLIENT_CAP_ASPECT_RATIO         4
#define DRM_CLIENT_CAP_WRITEBACK_CONNECTORS 5
#define DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT 6

struct drm_set_client_cap {
    __zn_u64 capability;
    __zn_u64 value;
};

struct drm_tex_region {
    __zn_u8 next;
    __zn_u8 prev;
    __zn_u8 in_use;
    __zn_u8 padding;
    __zn_u32 age;
};

#define DRM_ADD_COMMAND    0
#define DRM_RM_COMMAND     1
#define DRM_INST_HANDLER   2
#define DRM_UNINST_HANDLER 3

struct drm_control {
    __zn_u32 func;
    __zn_i32 irq;
};

typedef __zn_u32 drm_map_type;
#define DRM_FRAME_BUFFER   0
#define DRM_REGISTERS      1
#define DRM_SHM            2
#define DRM_AGP            3
#define DRM_SCATTER_GATHER 4
#define DRM_CONSISTENT     5

typedef __zn_u32 drm_map_flags;
#define DRM_RESTRICTED      0x01
#define DRM_READ_ONLY       0x02
#define DRM_LOCKED          0x04
#define DRM_KERNEL          0x08
#define DRM_WRITE_COMBINING 0x10
#define DRM_CONTAINS_LOCK   0x20
#define DRM_REMOVABLE       0x40
#define DRM_DRIVER          0x80

struct drm_ctx_priv_map {
    __zn_u32 ctx_id;
    __zn_uptr handle;
};

struct drm_map {
    __zn_size offset;
    __zn_size size;
    drm_map_type type;
    drm_map_flags flags;
    __zn_uptr handle;
    __zn_i32 mtrr;
};

struct drm_client {
    __zn_i32 idx;
    __zn_i32 auth;
    __zn_size pid;
    __zn_size uid;
    __zn_size magic;
    __zn_size iocs;
};

typedef __zn_u32 drm_stat_type;
#define DRM_STAT_LOCK      0
#define DRM_STAT_OPENS     1
#define DRM_STAT_CLOSES    2
#define DRM_STAT_IOCTLS    3
#define DRM_STAT_LOCKS     4
#define DRM_STAT_UNLOCKS   5
#define DRM_STAT_VALUE     6
#define DRM_STAT_BYTE      7
#define DRM_STAT_COUNT     8
#define DRM_STAT_IRQ       9
#define DRM_STAT_PRIMARY   10
#define DRM_STAT_SECONDARY 11
#define DRM_STAT_DMA       12
#define DRM_STAT_SPECIAL   13
#define DRM_STAT_MISSED    14

struct drm_stats_data {
    __zn_size value;
    drm_stat_type type;
};

struct drm_stats {
    __zn_size count;
    struct drm_stats_data data[15];
};

struct drm_draw {
    drm_drawable_t handle;
};

struct drm_set_version {
    __zn_i32 drm_di_major;
    __zn_i32 drm_di_minor;
    __zn_i32 drm_dd_major;
    __zn_i32 drm_dd_minor;
};

typedef __zn_u32 drm_ctx_flags;
#define DRM_CONTEXT_PRESERVED 0x01
#define DRM_CONTEXT_2DONLY    0x02

struct drm_ctx {
    drm_context_t handle;
    drm_ctx_flags flags;
};

struct drm_ctx_res {
    __zn_i32 count;
    __zn_uptr contexts;
};

#define DRM_DISPLAY_MODE_LEN 32

struct drm_mode_modeinfo {
    __zn_u32 clock;
    __zn_u16 hdisplay;
    __zn_u16 hsync_start;
    __zn_u16 hsync_end;
    __zn_u16 htotal;
    __zn_u16 hskew;
    __zn_u16 vdisplay;
    __zn_u16 vsync_start;
    __zn_u16 vsync_end;
    __zn_u16 vtotal;
    __zn_u16 vscan;
    __zn_u32 vrefresh;
    __zn_u32 flags;
    __zn_u32 type;
    char name[DRM_DISPLAY_MODE_LEN];
};

struct drm_mode_crtc {
    __zn_u64 set_connectors_ptr;
    __zn_u32 count_connectors;
    __zn_u32 crtc_id;
    __zn_u32 fb_id;
    __zn_u32 x;
    __zn_u32 y;
    __zn_u32 gamma_size;
    __zn_u32 mode_valid;
    struct drm_mode_modeinfo mode;
};

struct drm_mode_get_encoder {
    __zn_u32 encoder_id;
    __zn_u32 encoder_type;
    __zn_u32 crtc_id;
    __zn_u32 possible_crtcs;
    __zn_u32 possible_clones;
};

#define DRM_MODE_CONNECTOR_Unknown     0
#define DRM_MODE_CONNECTOR_VGA         1
#define DRM_MODE_CONNECTOR_DVII        2
#define DRM_MODE_CONNECTOR_DVID        3
#define DRM_MODE_CONNECTOR_DVIA        4
#define DRM_MODE_CONNECTOR_Composite   5
#define DRM_MODE_CONNECTOR_SVIDEO      6
#define DRM_MODE_CONNECTOR_LVDS        7
#define DRM_MODE_CONNECTOR_Component   8
#define DRM_MODE_CONNECTOR_9PinDIN     9
#define DRM_MODE_CONNECTOR_DisplayPort 10
#define DRM_MODE_CONNECTOR_HDMIA       11
#define DRM_MODE_CONNECTOR_HDMIB       12
#define DRM_MODE_CONNECTOR_TV          13
#define DRM_MODE_CONNECTOR_eDP         14
#define DRM_MODE_CONNECTOR_VIRTUAL     15
#define DRM_MODE_CONNECTOR_DSI         16
#define DRM_MODE_CONNECTOR_DPI         17
#define DRM_MODE_CONNECTOR_WRITEBACK   18
#define DRM_MODE_CONNECTOR_SPI         19
#define DRM_MODE_CONNECTOR_USB         20

#define DRM_MODE_CONNECTED         1
#define DRM_MODE_DISCONNECTED      2
#define DRM_MODE_UNKNOWNCONNECTION 3

struct drm_mode_get_connector {
    __zn_u64 encoders_ptr;
    __zn_u64 modes_ptr;
    __zn_u64 props_ptr;
    __zn_u64 prop_values_ptr;
    __zn_u32 count_modes;
    __zn_u32 count_props;
    __zn_u32 count_encoders;
    __zn_u32 encoder_id;
    __zn_u32 connector_id;
    __zn_u32 connector_type;
    __zn_u32 connector_type_id;
    __zn_u32 connection;
    __zn_u32 mm_width;
    __zn_u32 mm_height;
    __zn_u32 subpixel;
    __zn_u32 pad;
};

struct drm_mode_fb_cmd {
    __zn_u32 fb_id;
    __zn_u32 width;
    __zn_u32 height;
    __zn_u32 pitch;
    __zn_u32 bpp;
    __zn_u32 depth;
    __zn_u32 handle;
};

struct drm_mode_card_res {
    __zn_u64 fb_id_ptr;
    __zn_u64 crtc_id_ptr;
    __zn_u64 connector_id_ptr;
    __zn_u64 encoder_id_ptr;
    __zn_u32 count_fbs;
    __zn_u32 count_crtcs;
    __zn_u32 count_connectors;
    __zn_u32 count_encoders;
    __zn_u32 min_width;
    __zn_u32 max_width;
    __zn_u32 min_height;
    __zn_u32 max_height;
};

#define DRM_CLOEXEC __O_CLOEXEC
#define DRM_RDWR    __O_RDWR

struct drm_prime_handle {
    __zn_u32 handle;
    __zn_u32 flags;
    __zn_i32 fd;
};

struct drm_gem_close {
    __zn_u32 handle;
    __zn_u32 pad;
};

struct drm_gem_flink {
    __zn_u32 handle;
    __zn_u32 name;
};

struct drm_gem_open {
    __zn_u32 name;
    __zn_u32 handle;
    __zn_u64 size;
};

struct drm_mode_closefb {
    __zn_u32 fb_id;
    __zn_u32 pad;
};

struct drm_mode_crtc_lut {
    __zn_u32 crtc_id;
    __zn_u32 gamma_size;
    __zn_u64 red;
    __zn_u64 green;
    __zn_u64 blue;
};

struct drm_mode_list_lessees {
    __zn_u32 count_lessees;
    __zn_u32 pad;
    __zn_u64 lessees_ptr;
};

struct drm_mode_cursor {
    __zn_u32 flags;
    __zn_u32 crtc_id;
    __zn_i32 x;
    __zn_i32 y;
    __zn_u32 width;
    __zn_u32 height;
    __zn_u32 handle;
};

#define DRM_MODE_CURSOR_BO   0x01
#define DRM_MODE_CURSOR_MOVE 0x02

struct drm_mode_cursor2 {
    __zn_u32 flags;
    __zn_u32 crtc_id;
    __zn_i32 x;
    __zn_i32 y;
    __zn_u32 width;
    __zn_u32 height;
    __zn_u32 handle;
    __zn_i32 hot_x;
    __zn_i32 hot_y;
};

#define DRM_PROP_NAME_LEN          32
#define DRM_MODE_PROP_PENDING      (1U << 0)
#define DRM_MODE_PROP_RANGE        (1U << 1)
#define DRM_MODE_PROP_IMMUTABLE    (1U << 2)
#define DRM_MODE_PROP_ENUM         (1U << 3)
#define DRM_MODE_PROP_BLOB         (1U << 4)
#define DRM_MODE_PROP_BITMASK      (1U << 5)
#define DRM_MODE_PROP_OBJECT       (1U << 6)
#define DRM_MODE_PROP_SIGNED_RANGE (1U << 7)
#define DRM_MODE_PROP_ATOMIC       (1U << 31)

struct drm_mode_get_property {
    __zn_u64 values_ptr;
    __zn_u64 enum_blob_ptr;
    __zn_u32 prop_id;
    __zn_u32 flags;
    char name[DRM_PROP_NAME_LEN];
    __zn_u32 count_values;
    __zn_u32 count_enum_blobs;
};

struct drm_mode_connector_set_property {
    __zn_u64 value;
    __zn_u32 prop_id;
    __zn_u32 connector_id;
};

struct drm_mode_get_plane {
    __zn_u32 plane_id;
    __zn_u32 crtc_id;
    __zn_u32 fb_id;
    __zn_u32 possible_crtcs;
    __zn_u32 gamma_size;
    __zn_u32 count_format_types;
    __zn_u64 format_type_ptr;
};

#define DRM_PLANE_TYPE_OVERLAY 0
#define DRM_PLANE_TYPE_PRIMARY 1
#define DRM_PLANE_TYPE_CURSOR  2

struct drm_mode_get_plane_res {
    __zn_u64 plane_id_ptr;
    __zn_u32 count_planes;
};

struct drm_mode_create_dumb {
    __zn_u32 height;
    __zn_u32 width;
    __zn_u32 bpp;
    __zn_u32 flags;
    __zn_u32 handle;
    __zn_u32 pitch;
    __zn_u64 size;
};

struct drm_mode_get_blob {
    __zn_u32 blob_id;
    __zn_u32 length;
    __zn_u64 data;
};

struct drm_mode_crtc_page_flip {
    __zn_u32 crtc_id;
    __zn_u32 fb_id;
    __zn_u32 flags;
    __zn_u32 reserved;
    __zn_u64 user_data;
};

struct drm_crtc_get_sequence {
    __zn_u32 crtc_id;
    __zn_u32 active;
    __zn_u64 sequence;
    __zn_i64 sequence_ns;
};

struct drm_crtc_queue_sequence {
    __zn_u32 crtc_id;
    __zn_u32 flags;
    __zn_u64 sequence;
    __zn_u64 user_data;
};

struct drm_mode_fb_dirty_cmd {
    __zn_u32 fb_id;
    __zn_u32 flags;
    __zn_u32 color;
    __zn_u32 num_clips;
    __zn_u64 clips_ptr;
};

struct drm_mode_map_dumb {
    __zn_u32 handle;
    __zn_u32 pad;
    __zn_u64 offset;
};

struct drm_mode_destroy_dumb {
    __zn_u32 handle;
};

struct drm_mode_fb_cmd2 {
    __zn_u32 fb_id;
    __zn_u32 width;
    __zn_u32 height;
    __zn_u32 pixel_format;
    __zn_u32 flags;
    __zn_u32 handles[4];
    __zn_u32 pitches[4];
    __zn_u32 offsets[4];
    __zn_u64 modifier[4];
};

struct drm_mode_obj_get_properties {
    __zn_u64 props_ptr;
    __zn_u64 prop_values_ptr;
    __zn_u32 count_props;
    __zn_u32 obj_id;
    __zn_u32 obj_type;
};

struct drm_mode_atomic {
    __zn_u32 flags;
    __zn_u32 count_objs;
    __zn_u64 objs_ptr;
    __zn_u64 count_props_ptr;
    __zn_u64 props_ptr;
    __zn_u64 prop_values_ptr;
    __zn_u64 reserved;
    __zn_u64 user_data;
};

struct drm_mode_create_blob {
    __zn_u64 data;
    __zn_u32 length;
    __zn_u32 blob_id;
};

struct drm_mode_destroy_blob {
    __zn_u32 blob_id;
};

struct drm_mode_property_enum {
    __zn_u64 value;
    char name[32];
};

#define DRM_MODE_OBJECT_CRTC      0xcccccccc
#define DRM_MODE_OBJECT_CONNECTOR 0xc0c0c0c0
#define DRM_MODE_OBJECT_ENCODER   0xe0e0e0e0
#define DRM_MODE_OBJECT_MODE      0xdededede
#define DRM_MODE_OBJECT_PROPERTY  0xb0b0b0b0
#define DRM_MODE_OBJECT_FB        0xfbfbfbfb
#define DRM_MODE_OBJECT_BLOB      0xbbbbbbbb
#define DRM_MODE_OBJECT_PLANE     0xeeeeeeee
#define DRM_MODE_OBJECT_ANY       0

#define DRM_MODE_FLAG_PHSYNC    (1 << 0)
#define DRM_MODE_FLAG_NHSYNC    (1 << 1)
#define DRM_MODE_FLAG_PVSYNC    (1 << 2)
#define DRM_MODE_FLAG_NVSYNC    (1 << 3)
#define DRM_MODE_FLAG_INTERLACE (1 << 4)
#define DRM_MODE_TYPE_PREFERRED (1 << 3)
#define DRM_MODE_TYPE_DRIVER    (1 << 6)

#define DRM_IOCTL_VERSION                DRM_IOWR(0x00, struct drm_version)
#define DRM_IOCTL_GET_UNIQUE             DRM_IOWR(0x01, struct drm_unique)
#define DRM_IOCTL_GET_MAGIC              DRM_IOR(0x02, struct drm_auth)
/*#define DRM_IOCTL_IRQ_BUSID DRM_IOWR(0x03, struct drm_irq_busid)*/
#define DRM_IOCTL_GET_MAP                DRM_IOWR(0x04, struct drm_map)
#define DRM_IOCTL_GET_CLIENT             DRM_IOWR(0x05, struct drm_client)
#define DRM_IOCTL_GET_STATS              DRM_IOR(0x06, struct drm_stats)
#define DRM_IOCTL_SET_VERSION            DRM_IOWR(0x07, struct drm_set_version)
/*#define DRM_IOCTL_MODESET_CTL DRM_IOW(0x08, struct drm_modeset_ctl)*/
#define DRM_IOCTL_GEM_CLOSE              DRM_IOW(0x09, struct drm_gem_close)
#define DRM_IOCTL_GEM_FLINK              DRM_IOWR(0x0a, struct drm_gem_flink)
#define DRM_IOCTL_GEM_OPEN               DRM_IOWR(0x0b, struct drm_gem_open)
#define DRM_IOCTL_GET_CAP                DRM_IOWR(0x0c, struct drm_get_cap)
#define DRM_IOCTL_SET_CLIENT_CAP         DRM_IOW(0x0d, struct drm_set_client_cap)
#define DRM_IOCTL_SET_UNIQUE             DRM_IOW(0x10, struct drm_unique)
#define DRM_IOCTL_AUTH_MAGIC             DRM_IOW(0x11, struct drm_auth)
/*#define DRM_IOCTL_BLOCK DRM_IOWR(0x12, struct drm_block)*/
/*#define DRM_IOCTL_UNBLOCK DRM_IOWR(0x13, struct drm_block)*/
#define DRM_IOCTL_CONTROL                DRM_IOW(0x14, struct drm_control)
#define DRM_IOCTL_ADD_MAP                DRM_IOWR(0x15, struct drm_map)
/*#define DRM_IOCTL_ADD_BUFS DRM_IOWR(0x16, struct drm_buf_desc)*/
/*#define DRM_IOCTL_MARK_BUFS DRM_IOW(0x17, struct drm_buf_desc)*/
/*#define DRM_IOCTL_INFO_BUFS DRM_IOWR(0x18, struct drm_buf_info)*/
/*#define DRM_IOCTL_MAP_BUFS DRM_IOWR(0x19, struct drm_buf_map)*/
/*#define DRM_IOCTL_FREE_BUFS DRM_IOW(0x1a, struct drm_buf_free)*/
#define DRM_IOCTL_RM_MAP                 DRM_IOW(0x1b, struct drm_map)
/*#define DRM_IOCTL_SET_SAREA_CTX DRM_IOW(0x1c, struct drm_ctx_priv_map)*/
/*#define DRM_IOCTL_GET_SAREA_CTX DRM_IOWR(0x1d, struct drm_ctx_priv_map)*/
#define DRM_IOCTL_SET_MASTER             DRM_IO(0x1e)
#define DRM_IOCTL_DROP_MASTER            DRM_IO(0x1f)
#define DRM_IOCTL_ADD_CTX                DRM_IOWR(0x20, struct drm_ctx)
#define DRM_IOCTL_RM_CTX                 DRM_IOWR(0x21, struct drm_ctx)
#define DRM_IOCTL_MOD_CTX                DRM_IOW(0x22, struct drm_ctx)
#define DRM_IOCTL_GET_CTX                DRM_IOWR(0x23, struct drm_ctx)
#define DRM_IOCTL_SWITCH_CTX             DRM_IOW(0x24, struct drm_ctx)
#define DRM_IOCTL_NEW_CTX                DRM_IOW(0x25, struct drm_ctx)
#define DRM_IOCTL_RES_CTX                DRM_IOWR(0x26, struct drm_ctx_res)
#define DRM_IOCTL_ADD_DRAW               DRM_IOWR(0x27, struct drm_draw)
#define DRM_IOCTL_RM_DRAW                DRM_IOWR(0x28, struct drm_draw)
/*#define DRM_IOCTL_DMA DRM_IOWR(0x29, struct drm_dma)*/
/*#define DRM_IOCTL_LOCK DRM_IOW(0x2a, struct drm_lock)*/
/*#define DRM_IOCTL_UNLOCK DRM_IOW(0x2b, struct drm_lock)*/
/*#define DRM_IOCTL_FINISH DRM_IOW(0x2c, struct drm_lock)*/
#define DRM_IOCTL_PRIME_HANDLE_TO_FD     DRM_IOWR(0x2d, struct drm_prime_handle)
#define DRM_IOCTL_PRIME_FD_TO_HANDLE     DRM_IOWR(0x2e, struct drm_prime_handle)
/*#define DRM_IOCTL_AGP_ENABLE DRM_IOW(0x32, struct drm_agp_mode)*/
/*#define DRM_IOCTL_AGP_INFO DRM_IOR(0x33, struct drm_agp_info)*/
/*#define DRM_IOCTL_AGP_ALLOC DRM_IOWR(0x34, struct drm_agp_buffer)*/
/*#define DRM_IOCTL_AGP_FREE DRM_IOW(0x35, struct drm_agp_buffer)*/
/*#define DRM_IOCTL_AGP_BIND DRM_IOW(0x36, struct drm_agp_binding)*/
/*#define DRM_IOCTL_AGP_UNBIND DRM_IOW(0x37, struct drm_agp_binding)*/
/*#define DRM_IOCTL_SG_ALLOC DRM_IOWR(0x38, struct drm_scatter_gather)*/
/*#define DRM_IOCTL_SG_FREE DRM_IOW(0x39, struct drm_scatter_gather)*/
#define DRM_IOCTL_WAIT_VBLANK            DRM_IOWR(0x3a, __zn_u64[3])
#define DRM_IOCTL_CRTC_GET_SEQUENCE      DRM_IOWR(0x3b, struct drm_crtc_get_sequence)
#define DRM_IOCTL_CRTC_QUEUE_SEQUENCE    DRM_IOWR(0x3c, struct drm_crtc_queue_sequence)
/*#define DRM_IOCTL_UPDATE_DRAW DRM_IOW(0x3f, struct drm_update_draw)*/
#define DRM_IOCTL_MODE_GETRESOURCES      DRM_IOWR(0xA0, struct drm_mode_card_res)
#define DRM_IOCTL_MODE_GETCRTC           DRM_IOWR(0xA1, struct drm_mode_crtc)
#define DRM_IOCTL_MODE_SETCRTC           DRM_IOWR(0xA2, struct drm_mode_crtc)
#define DRM_IOCTL_MODE_CURSOR            DRM_IOWR(0xA3, struct drm_mode_cursor)
/*#define DRM_IOCTL_MODE_GETGAMMA DRM_IOWR(0xA4, struct drm_mode_crtc_lut)*/
#define DRM_IOCTL_MODE_SETGAMMA          DRM_IOWR(0xA5, struct drm_mode_crtc_lut)
#define DRM_IOCTL_MODE_GETENCODER        DRM_IOWR(0xA6, struct drm_mode_get_encoder)
#define DRM_IOCTL_MODE_GETCONNECTOR      DRM_IOWR(0xA7, struct drm_mode_get_connector)
#define DRM_IOCTL_MODE_GETPROPERTY       DRM_IOWR(0xAA, struct drm_mode_get_property)
#define DRM_IOCTL_MODE_SETPROPERTY       DRM_IOWR(0xAB, struct drm_mode_connector_set_property)
#define DRM_IOCTL_MODE_GETPROPBLOB       DRM_IOWR(0xAC, struct drm_mode_get_blob)
#define DRM_IOCTL_MODE_GETFB             DRM_IOWR(0xAD, struct drm_mode_fb_cmd)
#define DRM_IOCTL_MODE_ADDFB             DRM_IOWR(0xAE, struct drm_mode_fb_cmd)
#define DRM_IOCTL_MODE_RMFB              DRM_IOWR(0xAF, __zn_u32)
#define DRM_IOCTL_MODE_PAGE_FLIP         DRM_IOWR(0xB0, struct drm_mode_crtc_page_flip)
#define DRM_IOCTL_MODE_DIRTYFB           DRM_IOWR(0xB1, struct drm_mode_fb_dirty_cmd)
#define DRM_IOCTL_MODE_CREATE_DUMB       DRM_IOWR(0xB2, struct drm_mode_create_dumb)
#define DRM_IOCTL_MODE_MAP_DUMB          DRM_IOWR(0xB3, struct drm_mode_map_dumb)
#define DRM_IOCTL_MODE_DESTROY_DUMB      DRM_IOWR(0xB4, struct drm_mode_destroy_dumb)
#define DRM_IOCTL_MODE_GETPLANERESOURCES DRM_IOWR(0xB5, struct drm_mode_get_plane_res)
#define DRM_IOCTL_MODE_GETPLANE          DRM_IOWR(0xB6, struct drm_mode_get_plane)
/*#define DRM_IOCTL_MODE_SETPLANE DRM_IOWR(0xB7, struct drm_mode_set_plane)*/
#define DRM_IOCTL_MODE_ADDFB2            DRM_IOWR(0xB8, struct drm_mode_fb_cmd2)
#define DRM_IOCTL_MODE_OBJ_GETPROPERTIES DRM_IOWR(0xB9, struct drm_mode_obj_get_properties)
/*#define DRM_IOCTL_MODE_OBJ_SETPROPERTY DRM_IOWR(0xBA, struct drm_mode_obj_set_property)*/
#define DRM_IOCTL_MODE_CURSOR2           DRM_IOWR(0xBB, struct drm_mode_cursor2)
#define DRM_IOCTL_MODE_ATOMIC            DRM_IOWR(0xBC, struct drm_mode_atomic)
#define DRM_IOCTL_MODE_CREATEPROPBLOB    DRM_IOWR(0xBD, struct drm_mode_create_blob)
#define DRM_IOCTL_MODE_DESTROYPROPBLOB   DRM_IOWR(0xBE, struct drm_mode_destroy_blob)
/*#define DRM_IOCTL_SYNCOBJ_CREATE DRM_IOWR(0xBF, struct drm_syncobj_create)*/
/*#define DRM_IOCTL_SYNCOBJ_DESTROY DRM_IOWR(0xC0, struct drm_syncobj_destroy)*/
/*#define DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD DRM_IOWR(0xC1, struct drm_syncobj_handle)*/
/*#define DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE DRM_IOWR(0xC2, struct drm_syncobj_handle)*/
/*#define DRM_IOCTL_SYNCOBJ_WAIT DRM_IOWR(0xC3, struct drm_syncobj_wait)*/
/*#define DRM_IOCTL_SYNCOBJ_RESET DRM_IOWR(0xC4, struct drm_syncobj_array)*/
/*#define DRM_IOCTL_SYNCOBJ_SIGNAL DRM_IOWR(0xC5, struct drm_syncobj_array)*/
/*#define DRM_IOCTL_MODE_CREATE_LEASE DRM_IOWR(0xC6, struct drm_mode_create_lease)*/
#define DRM_IOCTL_MODE_LIST_LESSEES      DRM_IOWR(0xC7, struct drm_mode_list_lessees)
/*#define DRM_IOCTL_MODE_GET_LEASE DRM_IOWR(0xC8, struct drm_mode_get_lease)*/
/*#define DRM_IOCTL_MODE_REVOKE_LEASE DRM_IOWR(0xC9, struct drm_mode_revoke_lease)*/
/*#define DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT DRM_IOWR(0xCA, struct drm_syncobj_timeline_wait)*/
/*#define DRM_IOCTL_SYNCOBJ_QUERY DRM_IOWR(0xCB, struct drm_syncobj_timeline_array)*/
/*#define DRM_IOCTL_SYNCOBJ_TRANSFER DRM_IOWR(0xCC, struct drm_syncobj_transfer)*/
/*#define DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL DRM_IOWR(0xCD, struct drm_syncobj_timeline_array)*/
#define DRM_IOCTL_MODE_GETFB2            DRM_IOWR(0xCE, struct drm_mode_fb_cmd2)
/*#define DRM_IOCTL_SYNCOBJ_EVENTFD DRM_IOWR(0xCF, struct drm_syncobj_eventfd)*/
#define DRM_IOCTL_MODE_CLOSEFB           DRM_IOWR(0xD0, struct drm_mode_closefb)
/*#define DRM_IOCTL_SET_CLIENT_NAME DRM_IOWR(0xD1, struct drm_set_client_name)*/
/*#define DRM_IOCTL_GEM_CHANGE_HANDLE DRM_IOWR(0xD2, struct drm_gem_change_handle)*/

#define drm_fourcc_code(a, b, c, d) \
    ((__zn_u32)(a) | ((__zn_u32)(b) << 8) | ((__zn_u32)(c) << 16) | ((__zn_u32)(d) << 24))

#define DRM_FORMAT_BIG_ENDIAN           (1U << 31)
#define DRM_FORMAT_INVALID              0
#define DRM_FORMAT_C1                   drm_fourcc_code('C', '1', ' ', ' ')
#define DRM_FORMAT_C2                   drm_fourcc_code('C', '2', ' ', ' ')
#define DRM_FORMAT_C4                   drm_fourcc_code('C', '4', ' ', ' ')
#define DRM_FORMAT_C8                   drm_fourcc_code('C', '8', ' ', ' ')
#define DRM_FORMAT_D1                   drm_fourcc_code('D', '1', ' ', ' ')
#define DRM_FORMAT_D2                   drm_fourcc_code('D', '2', ' ', ' ')
#define DRM_FORMAT_D4                   drm_fourcc_code('D', '4', ' ', ' ')
#define DRM_FORMAT_D8                   drm_fourcc_code('D', '8', ' ', ' ')
#define DRM_FORMAT_R1                   drm_fourcc_code('R', '1', ' ', ' ')
#define DRM_FORMAT_R2                   drm_fourcc_code('R', '2', ' ', ' ')
#define DRM_FORMAT_R4                   drm_fourcc_code('R', '4', ' ', ' ')
#define DRM_FORMAT_R8                   drm_fourcc_code('R', '8', ' ', ' ')
#define DRM_FORMAT_R10                  drm_fourcc_code('R', '1', '0', ' ')
#define DRM_FORMAT_R12                  drm_fourcc_code('R', '1', '2', ' ')
#define DRM_FORMAT_R16                  drm_fourcc_code('R', '1', '6', ' ')
#define DRM_FORMAT_RG88                 drm_fourcc_code('R', 'G', '8', '8')
#define DRM_FORMAT_GR88                 drm_fourcc_code('G', 'R', '8', '8')
#define DRM_FORMAT_RG1616               drm_fourcc_code('R', 'G', '3', '2')
#define DRM_FORMAT_GR1616               drm_fourcc_code('G', 'R', '3', '2')
#define DRM_FORMAT_RGB332               drm_fourcc_code('R', 'G', 'B', '8')
#define DRM_FORMAT_BGR233               drm_fourcc_code('B', 'G', 'R', '8')
#define DRM_FORMAT_XRGB4444             drm_fourcc_code('X', 'R', '1', '2')
#define DRM_FORMAT_XBGR4444             drm_fourcc_code('X', 'B', '1', '2')
#define DRM_FORMAT_RGBX4444             drm_fourcc_code('R', 'X', '1', '2')
#define DRM_FORMAT_BGRX4444             drm_fourcc_code('B', 'X', '1', '2')
#define DRM_FORMAT_ARGB4444             drm_fourcc_code('A', 'R', '1', '2')
#define DRM_FORMAT_ABGR4444             drm_fourcc_code('A', 'B', '1', '2')
#define DRM_FORMAT_RGBA4444             drm_fourcc_code('R', 'A', '1', '2')
#define DRM_FORMAT_BGRA4444             drm_fourcc_code('B', 'A', '1', '2')
#define DRM_FORMAT_XRGB1555             drm_fourcc_code('X', 'R', '1', '5')
#define DRM_FORMAT_XBGR1555             drm_fourcc_code('X', 'B', '1', '5')
#define DRM_FORMAT_RGBX5551             drm_fourcc_code('R', 'X', '1', '5')
#define DRM_FORMAT_BGRX5551             drm_fourcc_code('B', 'X', '1', '5')
#define DRM_FORMAT_ARGB1555             drm_fourcc_code('A', 'R', '1', '5')
#define DRM_FORMAT_ABGR1555             drm_fourcc_code('A', 'B', '1', '5')
#define DRM_FORMAT_RGBA5551             drm_fourcc_code('R', 'A', '1', '5')
#define DRM_FORMAT_BGRA5551             drm_fourcc_code('B', 'A', '1', '5')
#define DRM_FORMAT_RGB565               drm_fourcc_code('R', 'G', '1', '6')
#define DRM_FORMAT_BGR565               drm_fourcc_code('B', 'G', '1', '6')
#define DRM_FORMAT_RGB888               drm_fourcc_code('R', 'G', '2', '4')
#define DRM_FORMAT_BGR888               drm_fourcc_code('B', 'G', '2', '4')
#define DRM_FORMAT_XRGB8888             drm_fourcc_code('X', 'R', '2', '4')
#define DRM_FORMAT_XBGR8888             drm_fourcc_code('X', 'B', '2', '4')
#define DRM_FORMAT_RGBX8888             drm_fourcc_code('R', 'X', '2', '4')
#define DRM_FORMAT_BGRX8888             drm_fourcc_code('B', 'X', '2', '4')
#define DRM_FORMAT_ARGB8888             drm_fourcc_code('A', 'R', '2', '4')
#define DRM_FORMAT_ABGR8888             drm_fourcc_code('A', 'B', '2', '4')
#define DRM_FORMAT_RGBA8888             drm_fourcc_code('R', 'A', '2', '4')
#define DRM_FORMAT_BGRA8888             drm_fourcc_code('B', 'A', '2', '4')
#define DRM_FORMAT_XRGB2101010          drm_fourcc_code('X', 'R', '3', '0')
#define DRM_FORMAT_XBGR2101010          drm_fourcc_code('X', 'B', '3', '0')
#define DRM_FORMAT_RGBX1010102          drm_fourcc_code('R', 'X', '3', '0')
#define DRM_FORMAT_BGRX1010102          drm_fourcc_code('B', 'X', '3', '0')
#define DRM_FORMAT_ARGB2101010          drm_fourcc_code('A', 'R', '3', '0')
#define DRM_FORMAT_ABGR2101010          drm_fourcc_code('A', 'B', '3', '0')
#define DRM_FORMAT_RGBA1010102          drm_fourcc_code('R', 'A', '3', '0')
#define DRM_FORMAT_BGRA1010102          drm_fourcc_code('B', 'A', '3', '0')
#define DRM_FORMAT_XRGB16161616         drm_fourcc_code('X', 'R', '4', '8')
#define DRM_FORMAT_XBGR16161616         drm_fourcc_code('X', 'B', '4', '8')
#define DRM_FORMAT_ARGB16161616         drm_fourcc_code('A', 'R', '4', '8')
#define DRM_FORMAT_ABGR16161616         drm_fourcc_code('A', 'B', '4', '8')
#define DRM_FORMAT_XRGB16161616F        drm_fourcc_code('X', 'R', '4', 'H')
#define DRM_FORMAT_XBGR16161616F        drm_fourcc_code('X', 'B', '4', 'H')
#define DRM_FORMAT_ARGB16161616F        drm_fourcc_code('A', 'R', '4', 'H')
#define DRM_FORMAT_ABGR16161616F        drm_fourcc_code('A', 'B', '4', 'H')
#define DRM_FORMAT_AXBXGXRX106106106106 drm_fourcc_code('A', 'B', '1', '0')
#define DRM_FORMAT_YUYV                 drm_fourcc_code('Y', 'U', 'Y', 'V')
#define DRM_FORMAT_YVYU                 drm_fourcc_code('Y', 'V', 'Y', 'U')
#define DRM_FORMAT_UYVY                 drm_fourcc_code('U', 'Y', 'V', 'Y')
#define DRM_FORMAT_VYUY                 drm_fourcc_code('V', 'Y', 'U', 'Y')
#define DRM_FORMAT_AYUV                 drm_fourcc_code('A', 'Y', 'U', 'V')
#define DRM_FORMAT_AVUY8888             drm_fourcc_code('A', 'V', 'U', 'Y')
#define DRM_FORMAT_XYUV8888             drm_fourcc_code('X', 'Y', 'U', 'V')
#define DRM_FORMAT_XVUY8888             drm_fourcc_code('X', 'V', 'U', 'Y')
#define DRM_FORMAT_VUY888               drm_fourcc_code('V', 'U', '2', '4')
#define DRM_FORMAT_VUY101010            drm_fourcc_code('V', 'U', '3', '0')
#define DRM_FORMAT_Y210                 drm_fourcc_code('Y', '2', '1', '0')
#define DRM_FORMAT_Y212                 drm_fourcc_code('Y', '2', '1', '2')
#define DRM_FORMAT_Y216                 drm_fourcc_code('Y', '2', '1', '6')
#define DRM_FORMAT_Y410                 drm_fourcc_code('Y', '4', '1', '0')
#define DRM_FORMAT_Y412                 drm_fourcc_code('Y', '4', '1', '2')
#define DRM_FORMAT_Y416                 drm_fourcc_code('Y', '4', '1', '6')
#define DRM_FORMAT_XVYU2101010          drm_fourcc_code('X', 'V', '3', '0')
#define DRM_FORMAT_XVYU12_16161616      drm_fourcc_code('X', 'V', '3', '6')
#define DRM_FORMAT_XVYU16161616         drm_fourcc_code('X', 'V', '4', '8')
#define DRM_FORMAT_Y0L0                 drm_fourcc_code('Y', '0', 'L', '0')
#define DRM_FORMAT_X0L0                 drm_fourcc_code('X', '0', 'L', '0')
#define DRM_FORMAT_Y0L2                 drm_fourcc_code('Y', '0', 'L', '2')
#define DRM_FORMAT_X0L2                 drm_fourcc_code('X', '0', 'L', '2')
#define DRM_FORMAT_YUV420_8BIT          drm_fourcc_code('Y', 'U', '0', '8')
#define DRM_FORMAT_YUV420_10BIT         drm_fourcc_code('Y', 'U', '1', '0')
#define DRM_FORMAT_XRGB8888_A8          drm_fourcc_code('X', 'R', 'A', '8')
#define DRM_FORMAT_XBGR8888_A8          drm_fourcc_code('X', 'B', 'A', '8')
#define DRM_FORMAT_RGBX8888_A8          drm_fourcc_code('R', 'X', 'A', '8')
#define DRM_FORMAT_BGRX8888_A8          drm_fourcc_code('B', 'X', 'A', '8')
#define DRM_FORMAT_RGB888_A8            drm_fourcc_code('R', '8', 'A', '8')
#define DRM_FORMAT_BGR888_A8            drm_fourcc_code('B', '8', 'A', '8')
#define DRM_FORMAT_RGB565_A8            drm_fourcc_code('R', '5', 'A', '8')
#define DRM_FORMAT_BGR565_A8            drm_fourcc_code('B', '5', 'A', '8')
#define DRM_FORMAT_NV12                 drm_fourcc_code('N', 'V', '1', '2')
#define DRM_FORMAT_NV21                 drm_fourcc_code('N', 'V', '2', '1')
#define DRM_FORMAT_NV16                 drm_fourcc_code('N', 'V', '1', '6')
#define DRM_FORMAT_NV61                 drm_fourcc_code('N', 'V', '6', '1')
#define DRM_FORMAT_NV24                 drm_fourcc_code('N', 'V', '2', '4')
#define DRM_FORMAT_NV42                 drm_fourcc_code('N', 'V', '4', '2')
#define DRM_FORMAT_NV15                 drm_fourcc_code('N', 'V', '1', '5')
#define DRM_FORMAT_NV20                 drm_fourcc_code('N', 'V', '2', '0')
#define DRM_FORMAT_NV30                 drm_fourcc_code('N', 'V', '3', '0')
#define DRM_FORMAT_P210                 drm_fourcc_code('P', '2', '1', '0')
#define DRM_FORMAT_P010                 drm_fourcc_code('P', '0', '1', '0')
#define DRM_FORMAT_P012                 drm_fourcc_code('P', '0', '1', '2')
#define DRM_FORMAT_P016                 drm_fourcc_code('P', '0', '1', '6')
#define DRM_FORMAT_P030                 drm_fourcc_code('P', '0', '3', '0')
#define DRM_FORMAT_Q410                 drm_fourcc_code('Q', '4', '1', '0')
#define DRM_FORMAT_Q401                 drm_fourcc_code('Q', '4', '0', '1')
#define DRM_FORMAT_YUV410               drm_fourcc_code('Y', 'U', 'V', '9')
#define DRM_FORMAT_YVU410               drm_fourcc_code('Y', 'V', 'U', '9')
#define DRM_FORMAT_YUV411               drm_fourcc_code('Y', 'U', '1', '1')
#define DRM_FORMAT_YVU411               drm_fourcc_code('Y', 'V', '1', '1')
#define DRM_FORMAT_YUV420               drm_fourcc_code('Y', 'U', '1', '2')
#define DRM_FORMAT_YVU420               drm_fourcc_code('Y', 'V', '1', '2')
#define DRM_FORMAT_YUV422               drm_fourcc_code('Y', 'U', '1', '6')
#define DRM_FORMAT_YVU422               drm_fourcc_code('Y', 'V', '1', '6')
#define DRM_FORMAT_YUV444               drm_fourcc_code('Y', 'U', '2', '4')
#define DRM_FORMAT_YVU444               drm_fourcc_code('Y', 'V', '2', '4')

#endif
