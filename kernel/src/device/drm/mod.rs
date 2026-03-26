use crate::{
    device::drm::object::{
        AtomicState, BufferObject, Connector, Crtc, Encoder, Framebuffer, ModeObject, Plane,
    },
    memory::{AddressSpace, UserPtr, VirtAddr, VmFlags},
    posix::errno::{EResult, Errno},
    uapi::{
        self,
        drm::{self, DRM_FORMAT_XRGB8888, drm_mode_modeinfo},
        poll::{POLLIN, POLLRDNORM},
    },
    util::mutex::spin::SpinMutex,
    vfs::{
        File,
        file::{FileOps, MmapFlags},
        fs::devtmpfs::register_device,
        inode::Mode,
    },
};
use alloc::{sync::Arc, vec::Vec};
use core::{
    num::NonZeroUsize,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

pub mod modes;
pub mod object;

mod plainfb;

pub struct IdAllocator {
    counter: AtomicU32,
}

impl IdAllocator {
    pub const fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
        }
    }

    pub fn alloc(&self) -> u32 {
        self.counter.fetch_add(1, Ordering::AcqRel)
    }
}

pub struct DeviceState {
    pub crtcs: SpinMutex<Vec<Arc<Crtc>>>,
    pub encoders: SpinMutex<Vec<Arc<Encoder>>>,
    pub connectors: SpinMutex<Vec<Arc<Connector>>>,
    pub planes: SpinMutex<Vec<Arc<Plane>>>,
    pub framebuffers: SpinMutex<Vec<Arc<Framebuffer>>>,
}

impl DeviceState {
    pub const fn new() -> Self {
        Self {
            crtcs: SpinMutex::new(Vec::new()),
            encoders: SpinMutex::new(Vec::new()),
            connectors: SpinMutex::new(Vec::new()),
            planes: SpinMutex::new(Vec::new()),
            framebuffers: SpinMutex::new(Vec::new()),
        }
    }
}

pub trait Device: Send + Sync {
    fn state(&self) -> &DeviceState;

    /// Returns a tuple of (major, minor, patch).
    fn driver_version(&self) -> (u32, u32, u32);

    /// Returns a tuple of (name, description, date).
    fn driver_info(&self) -> (&str, &str, &str);

    /// Creates a dumb framebuffer. Also returns the pitch in bytes.
    fn create_dumb(
        &self,
        file: &DrmFile,
        width: u32,
        height: u32,
        bpp: u32,
    ) -> EResult<(Arc<dyn BufferObject>, u32)> {
        let _ = (file, width, height, bpp);
        Err(Errno::ENOSYS)
    }

    fn create_fb(
        &self,
        file: &DrmFile,
        buffer: Arc<dyn BufferObject>,
        width: u32,
        height: u32,
        format: u32,
        pitch: u32,
    ) -> EResult<Arc<Framebuffer>> {
        let _ = (file, buffer, width, height, format, pitch);
        Err(Errno::ENOSYS)
    }

    fn commit(&self, state: &AtomicState);
}

/// Represents a user-facing DRM card in form of a file.
/// Each open() creates a new DrmFile with per-FD state.
pub struct DrmFile {
    device: Arc<dyn Device>,
    buffers: SpinMutex<Vec<Arc<dyn BufferObject>>>,
    active_fb: SpinMutex<Option<(u32, Arc<Framebuffer>)>>,
    events: SpinMutex<Vec<PageFlipEvent>>,
}

#[repr(C)]
struct PageFlipEvent {
    event_type: u32,
    length: u32,
    user_data: u64,
    tv_sec: u32,
    tv_usec: u32,
    sequence: u32,
    reserved: u32,
}

impl DrmFile {
    pub fn new(device: Arc<dyn Device>) -> Arc<Self> {
        Arc::new(Self {
            device,
            buffers: SpinMutex::new(Vec::new()),
            active_fb: SpinMutex::new(None),
            events: SpinMutex::new(Vec::new()),
        })
    }

    pub fn device(&self) -> &Arc<dyn Device> {
        &self.device
    }

    fn auto_flush(&self) {
        // Automatically flush the active framebuffer to handle apps that don't call DirtyFB
        if let Some((crtc_id, fb)) = self.active_fb.lock().clone() {
            let mut state = AtomicState::new(self.device.clone());
            state.set_crtc_framebuffer(crtc_id, fb);
            self.device.commit(&state);
        }
    }
}

impl FileOps for DrmFile {
    fn read(&self, _file: &File, buf: &mut [u8], _offset: u64) -> EResult<isize> {
        let mut events = self.events.lock();
        if events.is_empty() {
            return Err(Errno::EAGAIN);
        }

        let event = events.remove(0);
        let event_bytes = unsafe {
            core::slice::from_raw_parts(
                &event as *const PageFlipEvent as *const u8,
                core::mem::size_of::<PageFlipEvent>(),
            )
        };

        let copy_len = event_bytes.len().min(buf.len());
        buf[..copy_len].copy_from_slice(&event_bytes[..copy_len]);
        Ok(copy_len as isize)
    }

    fn poll(&self, _file: &File, mask: i16) -> EResult<i16> {
        let events = self.events.lock();
        let mut result = 0;

        // If there are events available, mark as readable
        if !events.is_empty() && (mask & (POLLIN | POLLRDNORM)) != 0 {
            result |= POLLIN | POLLRDNORM;
        }
        Ok(result)
    }

    fn ioctl(&self, _file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        match request as u32 {
            drm::DRM_IOCTL_SET_VERSION => {
                let mut ptr = UserPtr::<drm::drm_set_version>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_GET_CAP => {
                let mut ptr = UserPtr::<drm::drm_get_cap>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                match val.capability {
                    drm::DRM_CAP_DUMB_BUFFER => val.value = 1,
                    drm::DRM_CAP_ATOMIC => val.value = 1,
                    _ => warn!("Unknown Capability {}!", val.capability),
                }
                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_GETRESOURCES => {
                let mut ptr = UserPtr::<drm::drm_mode_card_res>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                let state = self.device.state();

                // Get CRTCs
                let crtcs = state.crtcs.lock();
                val.count_crtcs = crtcs.len() as _;
                let crtc_id_ptr = UserPtr::<u32>::new(val.crtc_id_ptr.into());
                if val.crtc_id_ptr != 0 {
                    for (i, crtc) in crtcs.iter().enumerate() {
                        crtc_id_ptr
                            .offset(i)
                            .write(crtc.id())
                            .ok_or(Errno::EFAULT)?;
                    }
                }

                // Get Encoders
                let encoders = state.encoders.lock();
                val.count_encoders = encoders.len() as _;
                let encoder_id_ptr = UserPtr::<u32>::new(val.encoder_id_ptr.into());
                if val.encoder_id_ptr != 0 {
                    for (i, encoders) in encoders.iter().enumerate() {
                        encoder_id_ptr
                            .offset(i)
                            .write(encoders.id())
                            .ok_or(Errno::EFAULT)?;
                    }
                }

                // Get Framebuffers
                let fbs = state.framebuffers.lock();
                val.count_fbs = fbs.len() as _;
                let fb_id_ptr = UserPtr::<u32>::new(val.fb_id_ptr.into());
                if val.fb_id_ptr != 0 {
                    for (i, fb) in fbs.iter().enumerate() {
                        fb_id_ptr.offset(i).write(fb.id()).ok_or(Errno::EFAULT)?;
                    }
                }

                // Get Connectors
                let conns = state.connectors.lock();
                val.count_connectors = conns.len() as _;
                let connector_id_ptr = UserPtr::<u32>::new(val.connector_id_ptr.into());
                if val.connector_id_ptr != 0 {
                    for (i, conn) in conns.iter().enumerate() {
                        connector_id_ptr
                            .offset(i)
                            .write(conn.id())
                            .ok_or(Errno::EFAULT)?;
                    }
                }

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_GETCONNECTOR => {
                let mut ptr = UserPtr::<drm::drm_mode_get_connector>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // Find the requested connector.
                let connectors = state.connectors.lock();
                let connector = connectors
                    .iter()
                    .find(|&x| x.id() == val.connector_id)
                    .ok_or(Errno::EINVAL)?;

                // Basic information about the connector.
                val.connection = connector.state as u32;
                val.connector_type = connector.connector_type as u32;
                val.connector_type_id = 0; // TODO

                // Modes
                if val.modes_ptr != 0 {
                    let modes_ptr = UserPtr::<drm_mode_modeinfo>::new(val.modes_ptr.into());
                    for (i, mode) in connector.modes.iter().enumerate() {
                        modes_ptr.offset(i).write(*mode).ok_or(Errno::EFAULT)?;
                    }
                }
                val.count_modes = connector.modes.len() as u32;

                // Encoders
                if val.encoders_ptr != 0 {
                    let encoders_ptr = UserPtr::<u32>::new(val.encoders_ptr.into());
                    for (i, encoder) in connector.possible_encoders.iter().enumerate() {
                        encoders_ptr
                            .offset(i)
                            .write(encoder.id())
                            .ok_or(Errno::EFAULT)?;
                    }
                }
                val.count_encoders = connector.possible_encoders.len() as u32;

                // TODO: Physical sizes
                val.mm_width = 0;
                val.mm_height = 0;
                val.subpixel = 0;

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_GETENCODER => {
                let mut ptr = UserPtr::<drm::drm_mode_get_encoder>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                let encoders = state.encoders.lock();
                let encoder = encoders
                    .iter()
                    .find(|&x| x.id() == val.encoder_id)
                    .ok_or(Errno::EINVAL)?;

                val.crtc_id = encoder.active_crtc.id() as _;

                // Create a bit mask using the IDs as indices.
                let mut possible_crtcs = 0;
                for crtc in encoder.possible_crtcs.iter() {
                    possible_crtcs |= 1 << crtc.id();
                }
                val.possible_crtcs = possible_crtcs;

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_GETPLANE => {
                let mut ptr = UserPtr::<drm::drm_mode_get_plane>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // Find the requested plane
                let planes = state.planes.lock();
                let plane = planes
                    .iter()
                    .find(|&x| x.id() == val.plane_id)
                    .ok_or(Errno::EINVAL)?;

                // Set basic plane info
                val.crtc_id = 0; // Not currently bound to a CRTC
                val.fb_id = 0; // Not currently displaying a framebuffer

                // Create bitmask of possible CRTCs using indices
                let crtcs = state.crtcs.lock();
                let mut possible_crtcs = 0u32;
                for possible_crtc in plane.possible_crtcs.iter() {
                    // Find the index of this CRTC in the global CRTC list
                    if let Some(idx) = crtcs.iter().position(|c| c.id() == possible_crtc.id()) {
                        possible_crtcs |= 1 << idx;
                    }
                }
                drop(crtcs);
                val.possible_crtcs = possible_crtcs;

                val.gamma_size = 0; // No gamma LUT support

                // Fill formats if user provided a buffer
                if val.format_type_ptr != 0 {
                    let format_ptr = UserPtr::<u32>::new(val.format_type_ptr.into());
                    for (i, &format) in plane.formats.iter().enumerate() {
                        format_ptr.offset(i).write(format).ok_or(Errno::EFAULT)?;
                    }
                }
                val.count_format_types = plane.formats.len() as u32;

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_GETPLANERESOURCES => {
                let mut ptr = UserPtr::<drm::drm_mode_get_plane_res>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                let planes = state.planes.lock();
                val.count_planes = planes.len() as u32;

                // Fill plane IDs if user provided a buffer
                if val.plane_id_ptr != 0 {
                    let plane_id_ptr = UserPtr::<u32>::new(val.plane_id_ptr.into());
                    for (i, plane) in planes.iter().enumerate() {
                        plane_id_ptr
                            .offset(i)
                            .write(plane.id())
                            .ok_or(Errno::EFAULT)?;
                    }
                }

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_CREATE_DUMB => {
                let mut ptr = UserPtr::<drm::drm_mode_create_dumb>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                let mut buffers = self.buffers.lock();

                let (buffer, pitch) = self
                    .device
                    .create_dumb(self, val.width, val.height, val.bpp)?;

                val.handle = buffer.id();
                val.pitch = pitch;
                val.size = buffer.size() as u64;

                buffers.push(buffer);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_MAP_DUMB => {
                let mut ptr = UserPtr::<drm::drm_mode_map_dumb>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                let buffers = self.buffers.lock();
                let buffer = buffers
                    .iter()
                    .find(|x| x.id() == val.handle)
                    .ok_or(Errno::EINVAL)?;

                val.offset = (buffer.id() as u64) << 32;

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_DESTROY_DUMB => {
                let ptr = UserPtr::<drm::drm_mode_destroy_dumb>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                let mut buffers = self.buffers.lock();
                let index = buffers
                    .iter()
                    .position(|x| x.id() == val.handle)
                    .ok_or(Errno::EINVAL)?;

                buffers.remove(index);
            }
            drm::DRM_IOCTL_MODE_ADDFB => {
                let mut ptr = UserPtr::<drm::drm_mode_fb_cmd>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                // Find the buffer object
                let buffers = self.buffers.lock();
                let buffer = buffers
                    .iter()
                    .find(|b| b.id() == val.handle)
                    .ok_or(Errno::EINVAL)?
                    .clone();
                drop(buffers);

                // Convert bpp/depth to a fourcc format
                // For now, assume XRGB8888 for 32bpp
                let fourcc = match val.bpp {
                    32 => DRM_FORMAT_XRGB8888,
                    _ => return Err(Errno::EINVAL),
                };

                // Create framebuffer
                let framebuffer = self
                    .device
                    .create_fb(self, buffer, val.width, val.height, fourcc, val.pitch)?;

                val.fb_id = framebuffer.id();
                self.device.state().framebuffers.lock().push(framebuffer);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_RMFB => {
                warn!("DRM_IOCTL_MODE_RMFB is a stub!");
                let mut ptr = UserPtr::<drm::drm_mode_fb_cmd>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                // TODO

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_GETCRTC => {
                warn!("DRM_IOCTL_MODE_GETCRTC is a stub!");
                let mut ptr = UserPtr::<drm::drm_mode_crtc>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                // TODO

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_SETCRTC => {
                let mut ptr = UserPtr::<drm::drm_mode_crtc>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // If fb_id is 0, this disables the CRTC
                if val.fb_id == 0 {
                    // TODO: Implement CRTC disable
                    ptr.write(val).ok_or(Errno::EFAULT)?;
                    return Ok(0);
                }

                // Validate CRTC exists
                {
                    let crtcs = state.crtcs.lock();
                    crtcs
                        .iter()
                        .find(|x| x.id() == val.crtc_id)
                        .ok_or(Errno::EINVAL)?;
                }

                // Validate framebuffer exists
                let fb = {
                    let framebuffers = state.framebuffers.lock();
                    framebuffers
                        .iter()
                        .find(|x| x.id == val.fb_id)
                        .ok_or(Errno::EINVAL)?
                        .clone()
                };

                // Store active framebuffer for auto-flush
                *self.active_fb.lock() = Some((val.crtc_id, fb.clone()));

                // Commit the new CRTC state with framebuffer
                let mut state = AtomicState::new(self.device.clone());
                state.set_crtc_framebuffer(val.crtc_id, fb);
                self.device.commit(&state);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_ATOMIC => {
                // Auto-flush before processing new atomic commit
                self.auto_flush();

                let ptr = UserPtr::<drm::drm_mode_atomic>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                // Read object IDs
                let objs_ptr = UserPtr::<u32>::new(val.objs_ptr.into());
                let count_props_ptr = UserPtr::<u32>::new(val.count_props_ptr.into());
                let props_ptr = UserPtr::<u32>::new(val.props_ptr.into());
                let prop_values_ptr = UserPtr::<u64>::new(val.prop_values_ptr.into());

                let state = AtomicState::new(self.device.clone());

                let mut prop_offset = 0;
                for i in 0..val.count_objs {
                    let _obj_id = objs_ptr.offset(i as usize).read().ok_or(Errno::EFAULT)?;
                    let prop_count = count_props_ptr
                        .offset(i as usize)
                        .read()
                        .ok_or(Errno::EFAULT)?;

                    for j in 0..prop_count {
                        let _prop_id = props_ptr
                            .offset((prop_offset + j) as usize)
                            .read()
                            .ok_or(Errno::EFAULT)?;
                        let _prop_value = prop_values_ptr
                            .offset((prop_offset + j) as usize)
                            .read()
                            .ok_or(Errno::EFAULT)?;

                        // Store property changes in state
                        // TODO: Implement property handling
                    }

                    prop_offset += prop_count;
                }

                // Check if this is a test-only commit
                const DRM_MODE_ATOMIC_TEST_ONLY: u32 = 0x0100;
                if val.flags & DRM_MODE_ATOMIC_TEST_ONLY == 0 {
                    // Actually commit the state
                    self.device.commit(&state);
                }
            }
            drm::DRM_IOCTL_SET_CLIENT_CAP => {
                let ptr = UserPtr::<drm::drm_set_client_cap>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                // Accept atomic modesetting capability
                match val.capability {
                    drm::DRM_CLIENT_CAP_ATOMIC => {
                        log!("SET_CLIENT_CAP: ATOMIC = {}", val.value);
                    }
                    drm::DRM_CLIENT_CAP_UNIVERSAL_PLANES => {
                        log!("SET_CLIENT_CAP: UNIVERSAL_PLANES = {}", val.value);
                    }
                    _ => {
                        warn!("Unknown client capability: {}", val.capability);
                    }
                }
            }
            drm::DRM_IOCTL_MODE_GETPROPERTY => {
                let mut ptr = UserPtr::<drm::drm_mode_get_property>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                // For now, we only support property ID 1 which is the "type" property for planes
                if val.prop_id == 1 {
                    // Copy "type" into the name field
                    let name_bytes = b"type";
                    val.name[..name_bytes.len()].copy_from_slice(name_bytes);
                    val.name[name_bytes.len()] = 0; // null terminate

                    // Property flags: this is an enum property with values
                    val.flags = 0x8; // DRM_MODE_PROP_ENUM
                    val.count_values = 0;

                    // Enum values for plane type
                    if val.enum_blob_ptr != 0 {
                        let mut enum_ptr = UserPtr::new(val.enum_blob_ptr.into());

                        // Type 0: Overlay
                        let mut overlay = drm::drm_mode_property_enum {
                            value: 0,
                            name: [0; 32],
                        };
                        overlay.name[..7].copy_from_slice(b"Overlay");
                        enum_ptr.write(overlay).ok_or(Errno::EFAULT)?;

                        // Type 1: Primary
                        let mut primary = drm::drm_mode_property_enum {
                            value: 1,
                            name: [0; 32],
                        };
                        primary.name[..7].copy_from_slice(b"Primary");
                        enum_ptr.offset(1).write(primary).ok_or(Errno::EFAULT)?;

                        // Type 2: Cursor
                        let mut cursor = drm::drm_mode_property_enum {
                            value: 2,
                            name: [0; 32],
                        };
                        cursor.name[..6].copy_from_slice(b"Cursor");
                        enum_ptr.offset(2).write(cursor).ok_or(Errno::EFAULT)?;
                    }
                    val.count_enum_blobs = 3; // 3 enum values
                } else {
                    return Err(Errno::EINVAL);
                }

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_OBJ_GETPROPERTIES => {
                let mut ptr = UserPtr::<drm::drm_mode_obj_get_properties>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // For now, return empty property lists since we don't have full property tracking yet
                // This allows atomic modesetting clients to query properties without failing

                match val.obj_type {
                    drm::DRM_MODE_OBJECT_CRTC => {
                        // Verify CRTC exists
                        let crtcs = state.crtcs.lock();
                        let _crtc = crtcs
                            .iter()
                            .find(|x| x.id() == val.obj_id)
                            .ok_or(Errno::EINVAL)?;
                        // Return empty property list for now
                        val.count_props = 0;
                    }
                    drm::DRM_MODE_OBJECT_CONNECTOR => {
                        // Verify connector exists
                        let connectors = state.connectors.lock();
                        let _conn = connectors
                            .iter()
                            .find(|x| x.id() == val.obj_id)
                            .ok_or(Errno::EINVAL)?;
                        // Return empty property list for now
                        val.count_props = 0;
                    }
                    drm::DRM_MODE_OBJECT_PLANE => {
                        // Verify plane exists
                        let planes = state.planes.lock();
                        let plane = planes
                            .iter()
                            .find(|x| x.id() == val.obj_id)
                            .ok_or(Errno::EINVAL)?;

                        log!(
                            "OBJ_GETPROPERTIES(PLANE): id={}, type={}",
                            val.obj_id,
                            plane.plane_type
                        );

                        // For now, we only expose the "type" property for planes
                        // Property ID 1 = "type", value = plane.plane_type
                        if val.props_ptr != 0 && val.prop_values_ptr != 0 {
                            let mut props_ptr = UserPtr::<u32>::new(val.props_ptr.into());
                            let mut values_ptr = UserPtr::<u64>::new(val.prop_values_ptr.into());

                            // Return the "type" property
                            props_ptr.write(1).ok_or(Errno::EFAULT)?; // property ID for "type"
                            values_ptr
                                .write(plane.plane_type as u64)
                                .ok_or(Errno::EFAULT)?;
                            log!("  -> prop_id=1, value={}", plane.plane_type);
                        }
                        val.count_props = 1; // We expose 1 property: "type"
                    }
                    drm::DRM_MODE_OBJECT_ENCODER => {
                        // Verify encoder exists
                        let encoders = state.encoders.lock();
                        let _enc = encoders
                            .iter()
                            .find(|x| x.id() == val.obj_id)
                            .ok_or(Errno::EINVAL)?;
                        // Return empty property list for now
                        val.count_props = 0;
                    }
                    _ => {
                        warn!("Unknown object type: {}", val.obj_type);
                        return Err(Errno::EINVAL);
                    }
                }

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_CREATEPROPBLOB => {
                let mut ptr = UserPtr::<drm::drm_mode_create_blob>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                // TODO: For now, just allocate a blob ID and ignore the data. Also just put a fake ID.
                let blob_id = 0;
                val.blob_id = blob_id;

                log!("CREATEPROPBLOB: length={}, blob_id={}", val.length, blob_id);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_PAGE_FLIP => {
                let ptr = UserPtr::<drm::drm_mode_crtc_page_flip>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // Find the framebuffer
                let framebuffers = state.framebuffers.lock();
                let fb = framebuffers
                    .iter()
                    .find(|x| x.id == val.fb_id)
                    .ok_or(Errno::ENOENT)?
                    .clone();
                drop(framebuffers);

                // Create an atomic state and commit it
                let mut state = AtomicState::new(self.device.clone());
                state.set_crtc_framebuffer(val.crtc_id, fb);
                self.device.commit(&state);

                // Queue page flip completion event if requested
                const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
                if val.flags & DRM_MODE_PAGE_FLIP_EVENT != 0 {
                    let event = PageFlipEvent {
                        event_type: 1, // DRM_EVENT_FLIP_COMPLETE
                        length: core::mem::size_of::<PageFlipEvent>() as u32,
                        user_data: val.user_data,
                        tv_sec: 0,
                        tv_usec: 0,
                        sequence: 0,
                        reserved: 0,
                    };
                    self.events.lock().push(event);
                }
            }
            x => {
                error!("Unknown ioctl {x:x}");
                return Err(Errno::ENOSYS);
            }
        }
        Ok(0)
    }

    fn mmap(
        &self,
        _file: &File,
        space: &mut AddressSpace,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        flags: MmapFlags,
        offset: uapi::off_t,
    ) -> EResult<VirtAddr> {
        if !flags.contains(MmapFlags::Shared) {
            return Err(Errno::EINVAL);
        }

        let buffer_id = ((offset as usize) >> 32) as u32;
        let buffers = self.buffers.lock();
        let buffer = buffers
            .iter()
            .find(|x| x.id() == buffer_id)
            .ok_or(Errno::EINVAL)?;

        space.map_object(
            buffer.clone(),
            addr,
            len,
            prot,
            offset as u32 as uapi::off_t,
        )?;

        Ok(addr)
    }
}

static CARD_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn register(card: Arc<DrmFile>) -> EResult<()> {
    log!("Registering new DRM card");
    register_device(
        format!("drmcard{}", CARD_COUNTER.fetch_add(1, Ordering::SeqCst)).as_bytes(),
        card,
        Mode::from_bits_truncate(0o660),
        false,
    )
}
