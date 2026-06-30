use crate::{
    device::drm::object::{
        AtomicState, BufferObject, Connector, Crtc, Encoder, Framebuffer, ModeObject, Plane,
    },
    memory::{AddressSpace, IovecIter, UserPtr, VirtAddr, VmFlags},
    posix::errno::{EResult, Errno},
    process::Identity,
    sched::Scheduler,
    uapi::{
        self,
        drm::{
            self, DRM_FORMAT_XRGB8888, DRM_MODE_CURSOR_BO, DRM_MODE_CURSOR_MOVE,
            drm_mode_connector_type, drm_mode_modeinfo,
        },
    },
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::{
        self, File,
        file::{FileDescription, FileOps, MmapFlags, OpenFlags, PollEventSet, PollFlags},
        fs::devtmpfs,
        inode::Mode,
    },
};
use alloc::{sync::Arc, vec::Vec};
use core::{
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
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
            counter: AtomicU32::new(1),
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
    connector_type_ids: [AtomicU32; core::mem::variant_count::<drm_mode_connector_type>()],
}

impl DeviceState {
    pub const fn new() -> Self {
        Self {
            crtcs: SpinMutex::new(Vec::new()),
            encoders: SpinMutex::new(Vec::new()),
            connectors: SpinMutex::new(Vec::new()),
            planes: SpinMutex::new(Vec::new()),
            framebuffers: SpinMutex::new(Vec::new()),
            connector_type_ids: [const { AtomicU32::new(0) }; _],
        }
    }

    pub fn next_connector_type_id(&self, connector_type: drm_mode_connector_type) -> u32 {
        self.connector_type_ids[connector_type as usize].fetch_add(1, Ordering::Relaxed) + 1
    }
}

pub trait Device: Send + Sync {
    fn state(&self) -> &DeviceState;

    /// Returns a tuple of (major, minor, patch).
    fn driver_version(&self) -> (i32, i32, i32);

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
        warn!(
            "drm::Device::create_dumb is not implemented for {}!",
            self.driver_info().0
        );
        Err(Errno::ENOTTY)
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
        warn!(
            "drm::Device::create_fb is not implemented for {}!",
            self.driver_info().0
        );
        Err(Errno::ENOTTY)
    }

    fn commit(&self, state: &AtomicState);

    /// Set the cursor image for a CRTC. `buffer` contains ARGB8888 pixel data.
    /// If `buffer` is None, the cursor should be hidden.
    /// `hot_x` and `hot_y` are the hotspot offsets (pixels from top-left of cursor image).
    fn set_cursor(
        &self,
        crtc_id: u32,
        buffer: Option<Arc<dyn BufferObject>>,
        width: u32,
        height: u32,
        hot_x: i32,
        hot_y: i32,
    ) -> EResult<()> {
        let _ = (buffer, crtc_id, width, height, hot_x, hot_y);
        Err(Errno::ENOTTY)
    }

    /// Move the cursor to a new position on the given CRTC.
    fn move_cursor(&self, crtc_id: u32, x: i32, y: i32) -> EResult<()> {
        let _ = (crtc_id, x, y);
        Err(Errno::ENOTTY)
    }
}

/// Represents a user-facing DRM card in form of a per-open file.
pub struct DrmFile {
    device: Arc<dyn Device>,
    buffers: SpinMutex<Vec<Arc<dyn BufferObject>>>,
    framebuffers: SpinMutex<Vec<Arc<Framebuffer>>>,
    active_fb: SpinMutex<Option<(u32, Arc<Framebuffer>)>>,
    events: SpinMutex<Vec<PageFlipEvent>>,
    rd_event: Event,
    flip_sequence: AtomicU32,
    atomic_cap: AtomicBool,
    universal_planes_cap: AtomicBool,
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
            framebuffers: SpinMutex::new(Vec::new()),
            active_fb: SpinMutex::new(None),
            events: SpinMutex::new(Vec::new()),
            rd_event: Event::new(),
            flip_sequence: AtomicU32::new(0),
            atomic_cap: AtomicBool::new(false),
            universal_planes_cap: AtomicBool::new(false),
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

    fn framebuffer_by_id(&self, id: u32) -> EResult<Arc<Framebuffer>> {
        self.framebuffers
            .lock()
            .iter()
            .find(|x| x.id == id)
            .cloned()
            .ok_or(Errno::EINVAL)
    }

    fn primary_plane(&self) -> Option<Arc<Plane>> {
        self.device
            .state()
            .planes
            .lock()
            .iter()
            .find(|p| p.plane_type == drm::DRM_PLANE_TYPE_PRIMARY)
            .cloned()
    }

    fn plane_visible(&self, plane: &Plane) -> bool {
        self.atomic_cap.load(Ordering::Relaxed)
            || self.universal_planes_cap.load(Ordering::Relaxed)
            || plane.plane_type == drm::DRM_PLANE_TYPE_OVERLAY
    }

    fn property_visible(&self, prop_id: u32) -> bool {
        let Some(info) = object::property_info(prop_id) else {
            return false;
        };
        self.atomic_cap.load(Ordering::Relaxed) || info.flags & drm::DRM_MODE_PROP_ATOMIC == 0
    }

    fn visible_prop_values(&self, props: Vec<(u32, u64)>) -> Vec<(u32, u64)> {
        props
            .into_iter()
            .filter(|(prop_id, _)| self.property_visible(*prop_id))
            .collect()
    }

    /// Point the primary plane at `fb`.
    fn set_primary_fb(&self, crtc_id: u32, fb: Option<Arc<Framebuffer>>) {
        if let Some(primary) = self.primary_plane() {
            let mut s = primary.state.lock();
            s.fb = fb;
            s.crtc_id = crtc_id;
        }
    }

    fn apply_atomic(&self, changes: &[(u32, u32, u64)]) -> EResult<()> {
        use object::{
            PROP_ACTIVE, PROP_CRTC_H, PROP_CRTC_ID, PROP_CRTC_W, PROP_CRTC_X, PROP_CRTC_Y,
            PROP_FB_ID, PROP_MODE_ID, PROP_SRC_H, PROP_SRC_W, PROP_SRC_X, PROP_SRC_Y,
        };
        let state = self.device.state();
        for &(obj_id, prop_id, value) in changes {
            let fb = if prop_id == PROP_FB_ID && value != 0 {
                Some(self.framebuffer_by_id(value as u32)?)
            } else {
                None
            };

            let plane = state
                .planes
                .lock()
                .iter()
                .find(|p| p.id() == obj_id)
                .cloned();
            if let Some(plane) = plane {
                let mut s = plane.state.lock();
                match prop_id {
                    PROP_FB_ID => s.fb = fb,
                    PROP_CRTC_ID => s.crtc_id = value as u32,
                    PROP_CRTC_X => s.crtc_x = value as i32,
                    PROP_CRTC_Y => s.crtc_y = value as i32,
                    PROP_CRTC_W => s.crtc_w = value as u32,
                    PROP_CRTC_H => s.crtc_h = value as u32,
                    PROP_SRC_X => s.src_x = value as u32,
                    PROP_SRC_Y => s.src_y = value as u32,
                    PROP_SRC_W => s.src_w = value as u32,
                    PROP_SRC_H => s.src_h = value as u32,
                    _ => {}
                }
                continue;
            }

            let crtc = state
                .crtcs
                .lock()
                .iter()
                .find(|c| c.id() == obj_id)
                .cloned();
            if let Some(crtc) = crtc {
                let mut a = crtc.atomic.lock();
                match prop_id {
                    PROP_ACTIVE => a.active = value as u32,
                    PROP_MODE_ID => a.mode_id = value as u32,
                    _ => {}
                }
                continue;
            }

            let conn = state
                .connectors
                .lock()
                .iter()
                .find(|c| c.id() == obj_id)
                .cloned();
            if let Some(conn) = conn
                && prop_id == PROP_CRTC_ID
            {
                *conn.crtc_id.lock() = value as u32;
            }
        }
        Ok(())
    }

    fn queue_flip_event(&self, user_data: u64) {
        let now = crate::clock::get_elapsed();
        let event = PageFlipEvent {
            event_type: 2, // DRM_EVENT_FLIP_COMPLETE
            length: core::mem::size_of::<PageFlipEvent>() as u32,
            user_data,
            tv_sec: now.as_secs() as u32,
            tv_usec: now.subsec_micros(),
            sequence: self.flip_sequence.fetch_add(1, Ordering::AcqRel),
            reserved: 0,
        };
        self.events.lock().push(event);
        self.rd_event.wake_all();
    }
}

impl Drop for DrmFile {
    fn drop(&mut self) {
        self.device.set_cursor(0, None, 0, 0, 0, 0).ok();
        self.events.lock().clear();
        self.active_fb.lock().take();
        self.buffers.lock().clear();

        let owned_ids = {
            let mut framebuffers = self.framebuffers.lock();
            let ids = framebuffers.iter().map(|x| x.id).collect::<Vec<_>>();
            framebuffers.clear();
            ids
        };

        self.device
            .state()
            .framebuffers
            .lock()
            .retain(|x| !owned_ids.contains(&x.id));
    }
}

struct DrmDeviceNode {
    device: Arc<dyn Device>,
    minor: u32,
}

impl crate::device::Device for DrmDeviceNode {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        Ok(DrmFile::new(self.device.clone()))
    }

    fn major(&self) -> u32 {
        226
    }

    fn minor(&self) -> u32 {
        self.minor
    }
}

struct PrimeBuffer {
    buffer: Arc<dyn BufferObject>,
}

impl FileOps for PrimeBuffer {
    fn mmap(
        &self,
        _file: &File,
        space: &mut AddressSpace,
        addr: VirtAddr,
        len: NonZeroUsize,
        prot: VmFlags,
        _flags: MmapFlags,
        _offset: uapi::off_t,
    ) -> EResult<VirtAddr> {
        space.map_object(self.buffer.clone(), addr, len, prot, 0)?;
        Ok(addr)
    }
}

impl FileOps for DrmFile {
    fn read(&self, file: &File, buf: &mut IovecIter, _offset: u64) -> EResult<isize> {
        loop {
            let guard = self.rd_event.guard();
            let event = {
                let mut events = self.events.lock();
                if events.is_empty() {
                    None
                } else {
                    Some(events.remove(0))
                }
            };

            if let Some(event) = event {
                let event_bytes = unsafe {
                    core::slice::from_raw_parts(
                        &event as *const PageFlipEvent as *const u8,
                        core::mem::size_of::<PageFlipEvent>(),
                    )
                };

                let copy_len = event_bytes.len().min(buf.len());
                buf.copy_from_slice(&event_bytes[..copy_len])?;
                return Ok(copy_len as isize);
            }

            if file.flags.lock().contains(OpenFlags::NonBlocking) {
                return Err(Errno::EAGAIN);
            }

            guard.wait();
            if crate::sched::Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let events = self.events.lock();
        let mut result = PollFlags::empty();

        // If there are events available, mark as readable
        if !events.is_empty() && mask.intersects(PollFlags::In | PollFlags::Rdnorm) {
            result |= PollFlags::In | PollFlags::Rdnorm;
        }
        Ok(result)
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        if mask.intersects(PollFlags::Read) {
            PollEventSet::one(&self.rd_event)
        } else {
            PollEventSet::new()
        }
    }

    fn ioctl(&self, _file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        match request as u32 {
            drm::DRM_IOCTL_VERSION => {
                let mut ptr = UserPtr::<drm::drm_version>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                (val.version_major, val.version_minor, val.version_patchlevel) =
                    self.device.driver_version();
                let (name, date, desc) = self.device.driver_info();

                if !val.name.is_null() && val.name_len > 0 {
                    let len = name.len().min(val.name_len);
                    val.name
                        .write_slice(&name.as_bytes()[..len])
                        .ok_or(Errno::EFAULT)?;
                }
                val.name_len = name.len();

                if !val.date.is_null() && val.date_len > 0 {
                    let len = date.len().min(val.date_len);
                    val.date
                        .write_slice(&date.as_bytes()[..len])
                        .ok_or(Errno::EFAULT)?;
                }
                val.date_len = date.len();

                if !val.desc.is_null() && val.desc_len > 0 {
                    let len = desc.len().min(val.desc_len);
                    val.desc
                        .write_slice(&desc.as_bytes()[..len])
                        .ok_or(Errno::EFAULT)?;
                }
                val.desc_len = desc.len();

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_SET_MASTER | drm::DRM_IOCTL_DROP_MASTER => {
                // No-op: single client, always master
            }
            drm::DRM_IOCTL_GET_MAGIC => {
                // TODO
                let mut ptr = UserPtr::<drm::drm_auth>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                val.magic = 1;
                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_AUTH_MAGIC => {
                // TODO
            }
            drm::DRM_IOCTL_PRIME_HANDLE_TO_FD => {
                let mut ptr = UserPtr::<drm::drm_prime_handle>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                let buffer = self
                    .buffers
                    .lock()
                    .iter()
                    .find(|b| b.id() == val.handle)
                    .ok_or(Errno::EINVAL)?
                    .clone();

                let ops: Arc<dyn FileOps> = Arc::new(PrimeBuffer { buffer });
                let file = File::open_disconnected(ops, OpenFlags::ReadWrite)?;

                let proc = Scheduler::get_current().get_process();
                let fd = proc
                    .open_files
                    .lock()
                    .open_file(
                        FileDescription {
                            file,
                            close_on_exec: AtomicBool::new(true),
                        },
                        0,
                    )
                    .ok_or(Errno::EMFILE)?;

                val.fd = fd;
                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_PRIME_FD_TO_HANDLE => {
                let mut ptr = UserPtr::<drm::drm_prime_handle>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                let proc = Scheduler::get_current().get_process();
                let file = proc
                    .open_files
                    .lock()
                    .get_fd(val.fd)
                    .ok_or(Errno::EBADF)?
                    .file;
                let prime =
                    Arc::downcast::<PrimeBuffer>(file.ops.clone()).map_err(|_| Errno::EINVAL)?;
                let buffer = prime.buffer.clone();
                val.handle = buffer.id();

                // Reattach so ADDFB/ADDFB2 can resolve the handle.
                let mut buffers = self.buffers.lock();
                if !buffers.iter().any(|b| b.id() == val.handle) {
                    buffers.push(buffer);
                }
                drop(buffers);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_GEM_CLOSE => {
                let ptr = UserPtr::<drm::drm_gem_close>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;
                self.buffers.lock().retain(|b| b.id() != val.handle);
            }
            drm::DRM_IOCTL_MODE_CLOSEFB => {
                let ptr = UserPtr::<drm::drm_mode_closefb>::new(arg);
                let fb_id = ptr.read().ok_or(Errno::EFAULT)?.fb_id;

                self.framebuffers.lock().retain(|x| x.id != fb_id);
                self.device
                    .state()
                    .framebuffers
                    .lock()
                    .retain(|x| x.id != fb_id);

                let mut active = self.active_fb.lock();
                if matches!(*active, Some((_, ref fb)) if fb.id == fb_id) {
                    *active = None;
                }
            }
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
                    drm::DRM_CAP_DUMB_PREFERRED_DEPTH => val.value = 24,
                    drm::DRM_CAP_DUMB_PREFER_SHADOW => val.value = 1,
                    drm::DRM_CAP_CURSOR_WIDTH | drm::DRM_CAP_CURSOR_HEIGHT => {
                        let has_cursor = self
                            .device
                            .state()
                            .planes
                            .lock()
                            .iter()
                            .any(|p| p.plane_type == drm::DRM_PLANE_TYPE_CURSOR);
                        if !has_cursor {
                            return Err(Errno::EINVAL);
                        }
                        val.value = 64;
                    }
                    drm::DRM_CAP_TIMESTAMP_MONOTONIC => val.value = 1,
                    drm::DRM_CAP_PRIME => {
                        val.value = drm::DRM_PRIME_CAP_IMPORT | drm::DRM_PRIME_CAP_EXPORT
                    }
                    _ => return Err(Errno::EINVAL),
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

                val.min_width = 1;
                val.max_width = 8192;
                val.min_height = 1;
                val.max_height = 8192;

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
                val.connector_type_id = connector.connector_type_id;
                val.encoder_id = connector.possible_encoders.first().map_or(0, |e| e.id());

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

                let props = self.visible_prop_values(connector.prop_values());
                if val.props_ptr != 0
                    && val.prop_values_ptr != 0
                    && (val.count_props as usize) >= props.len()
                {
                    let props_ptr = UserPtr::<u32>::new(val.props_ptr.into());
                    let values_ptr = UserPtr::<u64>::new(val.prop_values_ptr.into());
                    for (i, (id, value)) in props.iter().enumerate() {
                        props_ptr.offset(i).write(*id).ok_or(Errno::EFAULT)?;
                        values_ptr.offset(i).write(*value).ok_or(Errno::EFAULT)?;
                    }
                }
                val.count_props = props.len() as u32;

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

                // possible_crtcs is indexed by each CRTC's position in the global
                // CRTC list (drm_crtc_index), not by its object id.
                let crtcs = state.crtcs.lock();
                let mut possible_crtcs = 0u32;
                for crtc in encoder.possible_crtcs.iter() {
                    if let Some(idx) = crtcs.iter().position(|c| c.id() == crtc.id()) {
                        possible_crtcs |= 1 << idx;
                    }
                }
                drop(crtcs);
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
                if !self.plane_visible(plane) {
                    return Err(Errno::EINVAL);
                }

                let plane_state = plane.state.lock();
                val.crtc_id = plane_state.crtc_id;
                val.fb_id = plane_state.fb.as_ref().map_or(0, |fb| fb.id);
                drop(plane_state);

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
                let visible_planes: Vec<Arc<Plane>> = planes
                    .iter()
                    .filter(|plane| self.plane_visible(plane))
                    .cloned()
                    .collect();
                val.count_planes = visible_planes.len() as u32;

                // Fill plane IDs if user provided a buffer
                if val.plane_id_ptr != 0 {
                    let plane_id_ptr = UserPtr::<u32>::new(val.plane_id_ptr.into());
                    for (i, plane) in visible_planes.iter().enumerate() {
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

                ptr.write(val).ok_or(Errno::EFAULT)?;
                buffers.push(buffer);
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

                ptr.write(val).ok_or(Errno::EFAULT)?;
                self.framebuffers.lock().push(framebuffer.clone());
                self.device.state().framebuffers.lock().push(framebuffer);
            }
            drm::DRM_IOCTL_MODE_ADDFB2 => {
                let mut ptr = UserPtr::<drm::drm_mode_fb_cmd2>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                // We currently only support single-plane formats backed by a single dumb buffer.
                // Modifiers other than DRM_FORMAT_MOD_LINEAR(0) are rejected since the driver doesn't advertise them.
                if val.handles[0] == 0 {
                    return Err(Errno::EINVAL);
                }
                if val.handles[1] != 0 || val.handles[2] != 0 || val.handles[3] != 0 {
                    return Err(Errno::EINVAL);
                }
                if val.modifier[0] != 0 {
                    return Err(Errno::EINVAL);
                }

                let buffers = self.buffers.lock();
                let buffer = buffers
                    .iter()
                    .find(|b| b.id() == val.handles[0])
                    .ok_or(Errno::EINVAL)?
                    .clone();
                drop(buffers);

                let framebuffer = self.device.create_fb(
                    self,
                    buffer,
                    val.width,
                    val.height,
                    val.pixel_format,
                    val.pitches[0],
                )?;

                val.fb_id = framebuffer.id();

                ptr.write(val).ok_or(Errno::EFAULT)?;
                self.framebuffers.lock().push(framebuffer.clone());
                self.device.state().framebuffers.lock().push(framebuffer);
            }
            drm::DRM_IOCTL_MODE_RMFB => {
                let ptr = UserPtr::<u32>::new(arg);
                let fb_id = ptr.read().ok_or(Errno::EFAULT)?;

                let mut owned_framebuffers = self.framebuffers.lock();
                let index = owned_framebuffers
                    .iter()
                    .position(|x| x.id == fb_id)
                    .ok_or(Errno::ENOENT)?;
                owned_framebuffers.remove(index);
                drop(owned_framebuffers);

                let state = self.device.state();
                let mut framebuffers = state.framebuffers.lock();
                if let Some(index) = framebuffers.iter().position(|x| x.id == fb_id) {
                    framebuffers.remove(index);
                }

                // If this was the active framebuffer, clear it
                let mut active = self.active_fb.lock();
                if let Some((_, ref fb)) = *active {
                    if fb.id == fb_id {
                        *active = None;
                    }
                }
            }
            drm::DRM_IOCTL_MODE_GETCRTC => {
                let mut ptr = UserPtr::<drm::drm_mode_crtc>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // Validate CRTC exists
                {
                    let crtcs = state.crtcs.lock();
                    crtcs
                        .iter()
                        .find(|x| x.id() == val.crtc_id)
                        .ok_or(Errno::EINVAL)?;
                }

                // Fill in current state from active_fb
                let active = self.active_fb.lock();
                if let Some((crtc_id, ref fb)) = *active {
                    if crtc_id == val.crtc_id {
                        val.fb_id = fb.id;

                        // Check if there's a mode on the connector
                        let connectors = state.connectors.lock();
                        if let Some(conn) = connectors.first() {
                            if let Some(mode) = conn.modes.first() {
                                val.mode = *mode;
                                val.mode_valid = 1;
                            }
                        }
                    }
                } else {
                    val.fb_id = 0;
                    val.mode_valid = 0;
                }

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
                let fb = self.framebuffer_by_id(val.fb_id)?;

                // Store active framebuffer for auto-flush
                *self.active_fb.lock() = Some((val.crtc_id, fb.clone()));

                // Keep the primary plane state and legacy CRTC state in sync.
                self.set_primary_fb(val.crtc_id, Some(fb.clone()));
                let mut state = AtomicState::new(self.device.clone());
                state.set_crtc_framebuffer(val.crtc_id, fb);
                self.device.commit(&state);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_ATOMIC => {
                if !self.atomic_cap.load(Ordering::Relaxed) {
                    return Err(Errno::EINVAL);
                }

                // Auto-flush before processing new atomic commit
                self.auto_flush();

                let ptr = UserPtr::<drm::drm_mode_atomic>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                let objs_ptr = UserPtr::<u32>::new(val.objs_ptr.into());
                let count_props_ptr = UserPtr::<u32>::new(val.count_props_ptr.into());
                let props_ptr = UserPtr::<u32>::new(val.props_ptr.into());
                let prop_values_ptr = UserPtr::<u64>::new(val.prop_values_ptr.into());

                // Read user memory first before acquiring a lock.
                let mut changes: Vec<(u32, u32, u64)> = Vec::new();
                let mut prop_offset = 0u32;
                for i in 0..val.count_objs {
                    let obj_id = objs_ptr.offset(i as usize).read().ok_or(Errno::EFAULT)?;
                    let prop_count = count_props_ptr
                        .offset(i as usize)
                        .read()
                        .ok_or(Errno::EFAULT)?;
                    for j in 0..prop_count {
                        let prop_id = props_ptr
                            .offset((prop_offset + j) as usize)
                            .read()
                            .ok_or(Errno::EFAULT)?;
                        let value = prop_values_ptr
                            .offset((prop_offset + j) as usize)
                            .read()
                            .ok_or(Errno::EFAULT)?;
                        changes.push((obj_id, prop_id, value));
                    }
                    prop_offset += prop_count;
                }

                const DRM_MODE_ATOMIC_TEST_ONLY: u32 = 0x0100;
                const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;

                for &(_, prop_id, value) in &changes {
                    if prop_id == object::PROP_FB_ID && value != 0 {
                        self.framebuffer_by_id(value as u32)?;
                    }
                }

                // Test-only commits validate references without updating state.
                if val.flags & DRM_MODE_ATOMIC_TEST_ONLY != 0 {
                    return Ok(0);
                }

                self.apply_atomic(&changes)?;

                // Mirror the primary plane into the legacy CRTC commit path.
                let mut state = AtomicState::new(self.device.clone());
                if let Some(primary) = self.primary_plane() {
                    let s = primary.state.lock();
                    if let Some(fb) = &s.fb {
                        state.set_crtc_framebuffer(s.crtc_id, fb.clone());
                    }
                }
                self.device.commit(&state);

                if val.flags & DRM_MODE_PAGE_FLIP_EVENT != 0 {
                    self.queue_flip_event(val.user_data);
                }
            }
            drm::DRM_IOCTL_SET_CLIENT_CAP => {
                let ptr = UserPtr::<drm::drm_set_client_cap>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                // Accept atomic modesetting capability
                match val.capability {
                    drm::DRM_CLIENT_CAP_ATOMIC => {
                        self.atomic_cap.store(val.value != 0, Ordering::Relaxed);
                        log!("SET_CLIENT_CAP: ATOMIC = {}", val.value);
                    }
                    drm::DRM_CLIENT_CAP_UNIVERSAL_PLANES => {
                        self.universal_planes_cap
                            .store(val.value != 0, Ordering::Relaxed);
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

                let info = object::property_info(val.prop_id).ok_or(Errno::EINVAL)?;
                if info.flags & drm::DRM_MODE_PROP_ATOMIC != 0
                    && !self.atomic_cap.load(Ordering::Relaxed)
                {
                    return Err(Errno::EINVAL);
                }
                let value_capacity = val.count_values as usize;
                let enum_capacity = val.count_enum_blobs as usize;

                val.name.fill(0);
                let n = info.name.len().min(val.name.len() - 1);
                val.name[..n].copy_from_slice(&info.name[..n]);
                val.flags = info.flags;
                val.count_values = 0;
                val.count_enum_blobs = 0;

                match info.kind {
                    object::PropKind::Range(min, max) => {
                        val.count_values = 2;
                        if val.values_ptr != 0 && value_capacity >= 2 {
                            let mut vptr = UserPtr::<u64>::new(val.values_ptr.into());
                            vptr.write(min).ok_or(Errno::EFAULT)?;
                            vptr.offset(1).write(max).ok_or(Errno::EFAULT)?;
                        }
                    }
                    object::PropKind::SignedRange(min, max) => {
                        val.count_values = 2;
                        if val.values_ptr != 0 && value_capacity >= 2 {
                            let mut vptr = UserPtr::<u64>::new(val.values_ptr.into());
                            vptr.write(min as u64).ok_or(Errno::EFAULT)?;
                            vptr.offset(1).write(max as u64).ok_or(Errno::EFAULT)?;
                        }
                    }
                    object::PropKind::Object(obj_type) => {
                        val.count_values = 1;
                        if val.values_ptr != 0 && value_capacity >= 1 {
                            UserPtr::<u64>::new(val.values_ptr.into())
                                .write(obj_type as u64)
                                .ok_or(Errno::EFAULT)?;
                        }
                    }
                    object::PropKind::Blob => {}
                    object::PropKind::Enum(entries) => {
                        val.count_enum_blobs = entries.len() as u32;
                        if val.enum_blob_ptr != 0 && enum_capacity >= entries.len() {
                            let eptr = UserPtr::<drm::drm_mode_property_enum>::new(
                                val.enum_blob_ptr.into(),
                            );
                            for (i, (value, name)) in entries.iter().enumerate() {
                                let mut e = drm::drm_mode_property_enum {
                                    value: *value,
                                    name: [0; 32],
                                };
                                let m = name.len().min(e.name.len() - 1);
                                e.name[..m].copy_from_slice(&name[..m]);
                                eptr.offset(i).write(e).ok_or(Errno::EFAULT)?;
                            }
                        }
                    }
                }

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_OBJ_GETPROPERTIES => {
                let mut ptr = UserPtr::<drm::drm_mode_obj_get_properties>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                let state = self.device.state();

                // Collect (prop id, value) pairs and release the object locks before any userspace copy.
                let props: alloc::vec::Vec<(u32, u64)> = match val.obj_type {
                    drm::DRM_MODE_OBJECT_CRTC => state
                        .crtcs
                        .lock()
                        .iter()
                        .find(|x| x.id() == val.obj_id)
                        .ok_or(Errno::EINVAL)?
                        .prop_values(),
                    drm::DRM_MODE_OBJECT_CONNECTOR => state
                        .connectors
                        .lock()
                        .iter()
                        .find(|x| x.id() == val.obj_id)
                        .ok_or(Errno::EINVAL)?
                        .prop_values(),
                    drm::DRM_MODE_OBJECT_PLANE => state
                        .planes
                        .lock()
                        .iter()
                        .find(|x| x.id() == val.obj_id)
                        .ok_or(Errno::EINVAL)?
                        .prop_values(),
                    drm::DRM_MODE_OBJECT_ENCODER => {
                        state
                            .encoders
                            .lock()
                            .iter()
                            .find(|x| x.id() == val.obj_id)
                            .ok_or(Errno::EINVAL)?;
                        alloc::vec::Vec::new()
                    }
                    _ => {
                        warn!("Unknown object type: {}", val.obj_type);
                        return Err(Errno::EINVAL);
                    }
                };
                let props = self.visible_prop_values(props);

                // Only fill the arrays if the caller allocated enough room.
                if val.props_ptr != 0
                    && val.prop_values_ptr != 0
                    && (val.count_props as usize) >= props.len()
                {
                    let props_ptr = UserPtr::<u32>::new(val.props_ptr.into());
                    let values_ptr = UserPtr::<u64>::new(val.prop_values_ptr.into());
                    for (i, (id, value)) in props.iter().enumerate() {
                        props_ptr.offset(i).write(*id).ok_or(Errno::EFAULT)?;
                        values_ptr.offset(i).write(*value).ok_or(Errno::EFAULT)?;
                    }
                }
                val.count_props = props.len() as u32;

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_CREATEPROPBLOB => {
                let mut ptr = UserPtr::<drm::drm_mode_create_blob>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;

                // Blob IDs must be non-zero so property references remain valid.
                static BLOB_ID_COUNTER: AtomicU32 = AtomicU32::new(1);
                val.blob_id = BLOB_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            drm::DRM_IOCTL_MODE_DIRTYFB => {
                let ptr = UserPtr::<drm::drm_mode_fb_dirty_cmd>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                let fb = self
                    .framebuffer_by_id(val.fb_id)
                    .map_err(|_| Errno::ENOENT)?;

                // Find which CRTC is displaying this FB and flush it
                if let Some((crtc_id, _)) = self.active_fb.lock().as_ref() {
                    let mut commit = AtomicState::new(self.device.clone());
                    commit.set_crtc_framebuffer(*crtc_id, fb);
                    self.device.commit(&commit);
                }
            }
            drm::DRM_IOCTL_MODE_PAGE_FLIP => {
                let ptr = UserPtr::<drm::drm_mode_crtc_page_flip>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;
                let fb = self
                    .framebuffer_by_id(val.fb_id)
                    .map_err(|_| Errno::ENOENT)?;

                *self.active_fb.lock() = Some((val.crtc_id, fb.clone()));

                // Keep the primary plane state and legacy CRTC state in sync.
                self.set_primary_fb(val.crtc_id, Some(fb.clone()));
                let mut state = AtomicState::new(self.device.clone());
                state.set_crtc_framebuffer(val.crtc_id, fb);
                self.device.commit(&state);

                // Queue page flip completion event if requested
                const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
                if val.flags & DRM_MODE_PAGE_FLIP_EVENT != 0 {
                    self.queue_flip_event(val.user_data);
                }
            }
            drm::DRM_IOCTL_WAIT_VBLANK => {
                // TODO
                return Err(Errno::ENOTTY);
            }
            drm::DRM_IOCTL_CRTC_GET_SEQUENCE => {
                // TODO
                return Err(Errno::ENOTTY);
            }
            drm::DRM_IOCTL_CRTC_QUEUE_SEQUENCE => {
                // TODO
                return Err(Errno::ENOTTY);
            }
            drm::DRM_IOCTL_MODE_CURSOR => {
                let ptr = UserPtr::<drm::drm_mode_cursor>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                if val.flags & DRM_MODE_CURSOR_BO != 0 {
                    if val.handle == 0 {
                        // Hide cursor
                        self.device.set_cursor(val.crtc_id, None, 0, 0, 0, 0)?;
                    } else {
                        // Set cursor image
                        let buffer = {
                            let buffers = self.buffers.lock();
                            buffers
                                .iter()
                                .find(|b| b.id() == val.handle)
                                .ok_or(Errno::EINVAL)?
                                .clone()
                        };
                        self.device.set_cursor(
                            val.crtc_id,
                            Some(buffer),
                            val.width,
                            val.height,
                            0,
                            0,
                        )?;
                    }
                }
                if val.flags & DRM_MODE_CURSOR_MOVE != 0 {
                    self.device.move_cursor(val.crtc_id, val.x, val.y)?;
                }
            }
            drm::DRM_IOCTL_MODE_CURSOR2 => {
                let ptr = UserPtr::<drm::drm_mode_cursor2>::new(arg);
                let val = ptr.read().ok_or(Errno::EFAULT)?;

                if val.flags & DRM_MODE_CURSOR_BO != 0 {
                    if val.handle == 0 {
                        // Hide cursor
                        self.device.set_cursor(val.crtc_id, None, 0, 0, 0, 0)?;
                    } else {
                        // Set cursor image with hotspot
                        let buffer = {
                            let buffers = self.buffers.lock();
                            buffers
                                .iter()
                                .find(|b| b.id() == val.handle)
                                .ok_or(Errno::EINVAL)?
                                .clone()
                        };
                        self.device.set_cursor(
                            val.crtc_id,
                            Some(buffer),
                            val.width,
                            val.height,
                            val.hot_x,
                            val.hot_y,
                        )?;
                    }
                }
                if val.flags & DRM_MODE_CURSOR_MOVE != 0 {
                    self.device.move_cursor(val.crtc_id, val.x, val.y)?;
                }
            }
            drm::DRM_IOCTL_MODE_SETGAMMA => {
                warn!("DRM_IOCTL_MODE_SETGAMMA is a stub!");
                let ptr = UserPtr::<drm::drm_mode_crtc_lut>::new(arg);
                let _val = ptr.read().ok_or(Errno::EFAULT)?;
                // TODO: Implement gamma LUT support
            }
            drm::DRM_IOCTL_MODE_SETPROPERTY => {
                warn!("DRM_IOCTL_MODE_SETPROPERTY is a stub!");
                let ptr = UserPtr::<drm::drm_mode_connector_set_property>::new(arg);
                let _val = ptr.read().ok_or(Errno::EFAULT)?;
                // TODO: Implement connector property setting
            }
            drm::DRM_IOCTL_MODE_LIST_LESSEES => {
                let mut ptr = UserPtr::<drm::drm_mode_list_lessees>::new(arg);
                let mut val = ptr.read().ok_or(Errno::EFAULT)?;
                // No leases supported yet, return empty list
                val.count_lessees = 0;
                ptr.write(val).ok_or(Errno::EFAULT)?;
            }
            x => {
                error!("Unknown ioctl {x:x}");
                return Err(Errno::ENOTTY);
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

static CARD_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn register(device: Arc<dyn Device>) -> EResult<()> {
    let minor = CARD_COUNTER.fetch_add(1, Ordering::SeqCst);
    log!(
        "Registering new DRM card {} ({})",
        minor,
        device.driver_info().0
    );

    crate::device::register_char_node(
        format!("drm/card{}", minor).as_bytes(),
        Arc::new(DrmDeviceNode { device, minor }),
        Mode::from_bits_truncate(0o660),
    )
}

#[initgraph::task(
    name = "generic.device.drm",
    depends = [devtmpfs::DEVTMPFS_STAGE],
)]
pub fn INPUT_STAGE() {
    let root = devtmpfs::get_root();
    vfs::mkdir(
        root.clone(),
        root,
        b"drm",
        Mode::from_bits_truncate(0o755),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/drm/");
}
