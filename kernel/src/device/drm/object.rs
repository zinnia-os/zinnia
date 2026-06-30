use core::any::Any;

use crate::{
    device::drm::Device,
    memory::MemoryObject,
    uapi::drm::{
        DRM_MODE_OBJECT_CRTC, DRM_MODE_OBJECT_FB, DRM_MODE_PROP_ATOMIC, DRM_MODE_PROP_BLOB,
        DRM_MODE_PROP_ENUM, DRM_MODE_PROP_IMMUTABLE, DRM_MODE_PROP_OBJECT, DRM_MODE_PROP_RANGE,
        DRM_MODE_PROP_SIGNED_RANGE, drm_mode_connector_state, drm_mode_connector_type,
        drm_mode_modeinfo,
    },
    util::mutex::spin::SpinMutex,
};
use alloc::{collections::btree_map::BTreeMap, sync::Arc, vec, vec::Vec};

// Shared property IDs used by every object that exposes the same property.
pub const PROP_TYPE: u32 = 1;
pub const PROP_FB_ID: u32 = 2;
pub const PROP_CRTC_ID: u32 = 3;
pub const PROP_SRC_X: u32 = 4;
pub const PROP_SRC_Y: u32 = 5;
pub const PROP_SRC_W: u32 = 6;
pub const PROP_SRC_H: u32 = 7;
pub const PROP_CRTC_X: u32 = 8;
pub const PROP_CRTC_Y: u32 = 9;
pub const PROP_CRTC_W: u32 = 10;
pub const PROP_CRTC_H: u32 = 11;
pub const PROP_MODE_ID: u32 = 12;
pub const PROP_ACTIVE: u32 = 13;

pub enum PropKind {
    Enum(&'static [(u64, &'static [u8])]),
    Range(u64, u64),
    SignedRange(i64, i64),
    Object(u32),
    Blob,
}

pub struct PropInfo {
    pub name: &'static [u8],
    pub flags: u32,
    pub kind: PropKind,
}

/// Metadata for a property ID, returned by DRM_IOCTL_MODE_GETPROPERTY.
pub fn property_info(id: u32) -> Option<PropInfo> {
    Some(match id {
        PROP_TYPE => PropInfo {
            name: b"type",
            flags: DRM_MODE_PROP_ENUM | DRM_MODE_PROP_IMMUTABLE,
            kind: PropKind::Enum(&[(0, b"Overlay"), (1, b"Primary"), (2, b"Cursor")]),
        },
        PROP_FB_ID => PropInfo {
            name: b"FB_ID",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_OBJECT,
            kind: PropKind::Object(DRM_MODE_OBJECT_FB),
        },
        PROP_CRTC_ID => PropInfo {
            name: b"CRTC_ID",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_OBJECT,
            kind: PropKind::Object(DRM_MODE_OBJECT_CRTC),
        },
        PROP_SRC_X => PropInfo {
            name: b"SRC_X",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, u32::MAX as u64),
        },
        PROP_SRC_Y => PropInfo {
            name: b"SRC_Y",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, u32::MAX as u64),
        },
        PROP_SRC_W => PropInfo {
            name: b"SRC_W",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, u32::MAX as u64),
        },
        PROP_SRC_H => PropInfo {
            name: b"SRC_H",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, u32::MAX as u64),
        },
        PROP_CRTC_X => PropInfo {
            name: b"CRTC_X",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_SIGNED_RANGE,
            kind: PropKind::SignedRange(i32::MIN as i64, i32::MAX as i64),
        },
        PROP_CRTC_Y => PropInfo {
            name: b"CRTC_Y",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_SIGNED_RANGE,
            kind: PropKind::SignedRange(i32::MIN as i64, i32::MAX as i64),
        },
        PROP_CRTC_W => PropInfo {
            name: b"CRTC_W",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, u32::MAX as u64),
        },
        PROP_CRTC_H => PropInfo {
            name: b"CRTC_H",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, u32::MAX as u64),
        },
        PROP_MODE_ID => PropInfo {
            name: b"MODE_ID",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_BLOB,
            kind: PropKind::Blob,
        },
        PROP_ACTIVE => PropInfo {
            name: b"ACTIVE",
            flags: DRM_MODE_PROP_ATOMIC | DRM_MODE_PROP_RANGE,
            kind: PropKind::Range(0, 1),
        },
        _ => return None,
    })
}

pub trait ModeObject {
    fn id(&self) -> u32;
}

#[derive(Default)]
pub struct CrtcAtomic {
    pub active: u32,
    pub mode_id: u32,
}

pub struct Crtc {
    id: u32,
    pub atomic: SpinMutex<CrtcAtomic>,
}

impl Crtc {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            atomic: SpinMutex::new(CrtcAtomic::default()),
        }
    }

    /// (property id, current value) pairs for DRM_IOCTL_MODE_OBJ_GETPROPERTIES.
    pub fn prop_values(&self) -> Vec<(u32, u64)> {
        let a = self.atomic.lock();
        vec![
            (PROP_ACTIVE, a.active as u64),
            (PROP_MODE_ID, a.mode_id as u64),
        ]
    }
}

impl ModeObject for Crtc {
    fn id(&self) -> u32 {
        self.id
    }
}

pub struct Encoder {
    id: u32,
    pub possible_crtcs: Vec<Arc<Crtc>>,
    pub active_crtc: Arc<Crtc>,
}

impl Encoder {
    pub fn new(id: u32, possible_crtcs: Vec<Arc<Crtc>>, crtc: Arc<Crtc>) -> Self {
        Self {
            id,
            possible_crtcs,
            active_crtc: crtc,
        }
    }
}

impl ModeObject for Encoder {
    fn id(&self) -> u32 {
        self.id
    }
}

pub struct Connector {
    id: u32,
    pub state: drm_mode_connector_state,
    pub connector_type: drm_mode_connector_type,
    pub connector_type_id: u32,
    pub modes: Vec<drm_mode_modeinfo>,
    pub possible_encoders: Vec<Arc<Encoder>>,
    /// The CRTC selected by the connector's CRTC_ID property.
    pub crtc_id: SpinMutex<u32>,
}

impl Connector {
    pub fn new(
        id: u32,
        state: drm_mode_connector_state,
        modes: Vec<drm_mode_modeinfo>,
        possible_encoders: Vec<Arc<Encoder>>,
        connector_type: drm_mode_connector_type,
        connector_type_id: u32,
    ) -> Self {
        Self {
            id,
            state,
            connector_type,
            connector_type_id,
            modes,
            possible_encoders,
            crtc_id: SpinMutex::new(0),
        }
    }

    /// (property id, current value) pairs for DRM_IOCTL_MODE_OBJ_GETPROPERTIES.
    pub fn prop_values(&self) -> Vec<(u32, u64)> {
        vec![(PROP_CRTC_ID, *self.crtc_id.lock() as u64)]
    }
}

impl ModeObject for Connector {
    fn id(&self) -> u32 {
        self.id
    }
}

/// Mutable plane state set through atomic properties.
#[derive(Default, Clone)]
pub struct PlaneState {
    pub fb: Option<Arc<Framebuffer>>,
    pub crtc_id: u32,
    pub crtc_x: i32,
    pub crtc_y: i32,
    pub hot_x: i32,
    pub hot_y: i32,
    pub crtc_w: u32,
    pub crtc_h: u32,
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

pub struct Plane {
    pub id: u32,
    pub possible_crtcs: Vec<Arc<Crtc>>,
    pub plane_type: u32,   // 0=overlay, 1=primary, 2=cursor
    pub formats: Vec<u32>, // List of supported fourcc formats
    pub state: SpinMutex<PlaneState>,
}

impl Plane {
    pub fn new(
        id: u32,
        possible_crtcs: Vec<Arc<Crtc>>,
        plane_type: u32,
        formats: Vec<u32>,
    ) -> Self {
        Self {
            id,
            possible_crtcs,
            plane_type,
            formats,
            state: SpinMutex::new(PlaneState::default()),
        }
    }

    /// (property id, current value) pairs for DRM_IOCTL_MODE_OBJ_GETPROPERTIES.
    pub fn prop_values(&self) -> Vec<(u32, u64)> {
        let s = self.state.lock();
        vec![
            (PROP_TYPE, self.plane_type as u64),
            (PROP_FB_ID, s.fb.as_ref().map_or(0, |f| f.id) as u64),
            (PROP_CRTC_ID, s.crtc_id as u64),
            (PROP_SRC_X, s.src_x as u64),
            (PROP_SRC_Y, s.src_y as u64),
            (PROP_SRC_W, s.src_w as u64),
            (PROP_SRC_H, s.src_h as u64),
            (PROP_CRTC_X, s.crtc_x as i64 as u64),
            (PROP_CRTC_Y, s.crtc_y as i64 as u64),
            (PROP_CRTC_W, s.crtc_w as u64),
            (PROP_CRTC_H, s.crtc_h as u64),
        ]
    }
}

impl ModeObject for Plane {
    fn id(&self) -> u32 {
        self.id
    }
}

pub struct Framebuffer {
    pub id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    /// Amount of bytes in one line of pixels.
    pub pitch: u32,
    /// Amount of bytes between the start of the buffer and the first pixel in the buffer.
    pub offset: u32,
    /// Backing buffer object
    pub buffer: Arc<dyn BufferObject>,
}

impl ModeObject for Framebuffer {
    fn id(&self) -> u32 {
        self.id
    }
}

pub trait BufferObject: MemoryObject + Any + Send + Sync {
    fn id(&self) -> u32;
    fn size(&self) -> usize;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
}

pub struct CrtcState {
    pub framebuffer: Option<Arc<Framebuffer>>,
}

pub struct ConnectorState {}

pub struct AtomicState {
    _device: Arc<dyn Device>,
    pub crtc_states: BTreeMap<u32, Arc<CrtcState>>,
    pub connector_states: BTreeMap<u32, Arc<ConnectorState>>,
}

impl AtomicState {
    pub const fn new(device: Arc<dyn Device>) -> Self {
        Self {
            _device: device,
            crtc_states: BTreeMap::new(),
            connector_states: BTreeMap::new(),
        }
    }

    pub fn set_crtc_framebuffer(&mut self, crtc_id: u32, framebuffer: Arc<Framebuffer>) {
        let state = Arc::new(CrtcState {
            framebuffer: Some(framebuffer),
        });
        self.crtc_states.insert(crtc_id, state);
    }
}
