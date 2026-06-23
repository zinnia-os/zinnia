use core::any::Any;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{
    device::drm::Device,
    memory::MemoryObject,
    uapi::drm::{drm_mode_connector_state, drm_mode_connector_type, drm_mode_modeinfo},
};
use alloc::{collections::btree_map::BTreeMap, sync::Arc, vec::Vec};

static CONNECTOR_TYPE_IDS: [AtomicU32; core::mem::variant_count::<drm_mode_connector_type>()] =
    [const { AtomicU32::new(0) }; _];

fn id_for_type(connector_type: drm_mode_connector_type) -> u32 {
    CONNECTOR_TYPE_IDS[connector_type as usize].fetch_add(1, Ordering::Relaxed) + 1
}

pub trait ModeObject {
    fn id(&self) -> u32;
}

pub struct Crtc {
    id: u32,
}

impl Crtc {
    pub fn new(id: u32) -> Self {
        Self { id }
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
}

impl Connector {
    pub fn new(
        id: u32,
        state: drm_mode_connector_state,
        modes: Vec<drm_mode_modeinfo>,
        possible_encoders: Vec<Arc<Encoder>>,
        connector_type: drm_mode_connector_type,
    ) -> Self {
        Self {
            id,
            state,
            connector_type,
            connector_type_id: id_for_type(connector_type),
            modes,
            possible_encoders,
        }
    }
}

impl ModeObject for Connector {
    fn id(&self) -> u32 {
        self.id
    }
}

pub struct Plane {
    pub id: u32,
    pub possible_crtcs: Vec<Arc<Crtc>>,
    pub plane_type: u32,   // 0=overlay, 1=primary, 2=cursor
    pub formats: Vec<u32>, // List of supported fourcc formats
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
        }
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
