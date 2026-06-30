use crate::spec::*;
use core::any::Any;
use virtio::{VirtQueue, VirtioDevice};
use zinnia::{
    alloc::{sync::Arc, vec, vec::Vec},
    arch,
    core::sync::atomic::{AtomicU32, Ordering},
    device::drm::{
        Device, DeviceState, DrmFile, IdAllocator,
        modes::{DMT_MODES, synthesize_preferred_mode},
        object::{AtomicState, BufferObject, Connector, Crtc, Encoder, Framebuffer, Plane},
    },
    error, log,
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr},
    posix::errno::{EResult, Errno},
    uapi::drm::{
        DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_PLANE_TYPE_CURSOR, drm_mode_connector_state,
    },
    util::mutex::spin::SpinMutex,
};
use zinnia::{memory::OwnedPhysPages, uapi::drm::drm_mode_connector_type};

pub struct VirtioGpuDevice {
    state: DeviceState,
    virtio: Arc<SpinMutex<VirtioDevice>>,
    ctrl_queue: Arc<SpinMutex<VirtQueue>>,
    cursor_queue: Arc<SpinMutex<VirtQueue>>,
    resource_id_counter: AtomicU32,
    scanouts: SpinMutex<Vec<ScanoutInfo>>,
    active_resource: AtomicU32, // Track which resource is active
    obj_counter: IdAllocator,
    cursor_resource: AtomicU32, // Resource ID of current cursor image (0 = none)
    cursor_x: AtomicU32,
    cursor_y: AtomicU32,
    cursor_hot_x: AtomicU32,
    cursor_hot_y: AtomicU32,
}

struct ScanoutInfo {
    id: u32,
    width: u32,
    height: u32,
    current_resource: Option<u32>,
}

impl VirtioGpuDevice {
    pub fn new(
        virtio: VirtioDevice,
        ctrl_queue: SpinMutex<VirtQueue>,
        cursor_queue: SpinMutex<VirtQueue>,
    ) -> EResult<Self> {
        let device = Self {
            state: DeviceState::new(),
            virtio: Arc::new(SpinMutex::new(virtio)),
            ctrl_queue: Arc::new(ctrl_queue),
            cursor_queue: Arc::new(cursor_queue),
            resource_id_counter: AtomicU32::new(1),
            scanouts: SpinMutex::new(Vec::new()),
            active_resource: AtomicU32::new(0),
            obj_counter: IdAllocator::new(),
            cursor_resource: AtomicU32::new(0),
            cursor_x: AtomicU32::new(0),
            cursor_y: AtomicU32::new(0),
            cursor_hot_x: AtomicU32::new(0),
            cursor_hot_y: AtomicU32::new(0),
        };

        // Get display info
        device.get_display_info()?;

        Ok(device)
    }

    fn alloc_resource_id(&self) -> u32 {
        self.resource_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    fn send_ctrl_command<T: Copy, R: Copy>(
        virtio: &SpinMutex<VirtioDevice>,
        ctrl_queue: &SpinMutex<VirtQueue>,
        cmd: &T,
    ) -> EResult<R> {
        let page_size = arch::virt::get_page_size();
        let cmd_size = core::mem::size_of::<T>();
        let cmd_pages = cmd_size.div_ceil(page_size);
        let cmd_buffer = OwnedPhysPages::new(cmd_pages, AllocFlags::empty())?;
        let cmd_ptr = cmd_buffer.as_hhdm::<T>();
        unsafe {
            core::ptr::write_volatile(cmd_ptr, *cmd);
        }

        // Allocate response buffer
        let resp = OwnedPhysPages::new(1, AllocFlags::empty())?;
        let resp_ptr = resp.as_hhdm::<R>();

        let buffers = vec![
            (cmd_buffer.phys(), cmd_size, false),
            (resp.phys(), core::mem::size_of::<R>(), true),
        ];

        {
            let mut queue = ctrl_queue.lock();
            queue.add_buffer(&buffers)?;
            virtio.lock().notify_queue(&queue);
        }

        // Wait for response
        loop {
            let mut queue = ctrl_queue.lock();
            if let Some((desc_id, _)) = queue.get_used() {
                queue.release_used_chain(desc_id);
                break;
            }
            drop(queue);

            core::hint::spin_loop();
        }

        let response = unsafe { core::ptr::read_volatile(resp_ptr) };
        Ok(response)
    }

    fn send_command<T: Copy, R: Copy>(&self, cmd: &T) -> EResult<R> {
        Self::send_ctrl_command(&self.virtio, &self.ctrl_queue, cmd)
    }

    fn get_display_info(&self) -> EResult<()> {
        let cmd = VirtioGpuGetDisplayInfo {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_GET_DISPLAY_INFO),
        };

        let resp: VirtioGpuRespDisplayInfo = self.send_command(&cmd)?;

        if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
            error!("Failed to get display info: {:?}", resp.hdr);
            return Err(Errno::EIO);
        }

        let mut scanouts = self.scanouts.lock();
        for (i, pmode) in resp.pmodes.iter().enumerate() {
            if pmode.enabled != 0 {
                // Use the device's native resolution
                let width = pmode.r.width;
                let height = pmode.r.height;
                scanouts.push(ScanoutInfo {
                    id: i as u32,
                    width,
                    height,
                    current_resource: None,
                });
            }
        }

        if scanouts.is_empty() {
            error!("No enabled scanouts found");
            return Err(Errno::ENODEV);
        }

        Ok(())
    }

    pub fn create_resource_2d(
        &self,
        resource_id: u32,
        width: u32,
        height: u32,
        format: u32,
    ) -> EResult<()> {
        let cmd = VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_CREATE_2D),
            resource_id,
            format,
            width,
            height,
        };

        let resp: VirtioGpuCtrlHdr = self.send_command(&cmd)?;

        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            error!(
                "Failed to create 2D resource (response type=0x{:x})",
                resp.type_
            );
            return Err(Errno::EIO);
        }
        Ok(())
    }

    fn detach_backing_raw(
        virtio: &SpinMutex<VirtioDevice>,
        ctrl_queue: &SpinMutex<VirtQueue>,
        resource_id: u32,
    ) -> EResult<()> {
        let cmd = VirtioGpuResourceDetachBacking {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING),
            resource_id,
            padding: 0,
        };

        let resp: VirtioGpuCtrlHdr = Self::send_ctrl_command(virtio, ctrl_queue, &cmd)?;
        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            return Err(Errno::EIO);
        }
        Ok(())
    }

    fn unref_resource_raw(
        virtio: &SpinMutex<VirtioDevice>,
        ctrl_queue: &SpinMutex<VirtQueue>,
        resource_id: u32,
    ) -> EResult<()> {
        let cmd = VirtioGpuResourceUnref {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_UNREF),
            resource_id,
            padding: 0,
        };

        let resp: VirtioGpuCtrlHdr = Self::send_ctrl_command(virtio, ctrl_queue, &cmd)?;
        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            return Err(Errno::EIO);
        }
        Ok(())
    }

    fn destroy_resource_raw(
        virtio: &SpinMutex<VirtioDevice>,
        ctrl_queue: &SpinMutex<VirtQueue>,
        resource_id: u32,
        has_backing: bool,
    ) {
        if has_backing {
            Self::detach_backing_raw(virtio, ctrl_queue, resource_id).ok();
        }
        Self::unref_resource_raw(virtio, ctrl_queue, resource_id).ok();
    }

    pub fn attach_backing(&self, resource_id: u32, pages: &[PhysAddr]) -> EResult<()> {
        let page_size = arch::virt::get_page_size();
        // Allocate command buffer for header + memory entries
        let cmd_size = core::mem::size_of::<VirtioGpuResourceAttachBacking>()
            + pages.len() * core::mem::size_of::<VirtioGpuMemEntry>();
        let cmd_pages = cmd_size.div_ceil(page_size);
        let cmd = OwnedPhysPages::new(cmd_pages, AllocFlags::empty())?;
        let cmd_ptr = cmd.as_hhdm::<u8>();

        unsafe {
            let hdr = cmd_ptr as *mut VirtioGpuResourceAttachBacking;
            (*hdr).hdr = VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
            (*hdr).resource_id = resource_id;
            (*hdr).nr_entries = pages.len() as u32;

            let entries_ptr = cmd_ptr.add(core::mem::size_of::<VirtioGpuResourceAttachBacking>())
                as *mut VirtioGpuMemEntry;
            for (i, &page_addr) in pages.iter().enumerate() {
                let entry = &mut *entries_ptr.add(i);
                entry.addr = page_addr.value() as u64;
                entry.length = page_size as u32;
                entry.padding = 0;
            }
        }

        // Allocate response buffer
        let resp = OwnedPhysPages::new(1, AllocFlags::empty())?;
        let resp_ptr = resp.as_hhdm::<VirtioGpuCtrlHdr>();

        let buffers = vec![
            (cmd.phys(), cmd_size, false),
            (resp.phys(), core::mem::size_of::<VirtioGpuCtrlHdr>(), true),
        ];

        {
            let mut queue = self.ctrl_queue.lock();
            queue.add_buffer(&buffers)?;

            self.virtio.lock().notify_queue(&queue);
        }

        // Wait for response
        loop {
            let mut queue = self.ctrl_queue.lock();
            if let Some((desc_id, _)) = queue.get_used() {
                queue.release_used_chain(desc_id);
                break;
            }
            drop(queue);
            core::hint::spin_loop();
        }

        let resp = unsafe { core::ptr::read_volatile(resp_ptr) };
        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            error!("Failed to attach backing");
            return Err(Errno::EIO);
        }

        Ok(())
    }

    pub fn set_scanout(
        &self,
        scanout_id: u32,
        resource_id: u32,
        width: u32,
        height: u32,
    ) -> EResult<()> {
        let cmd = VirtioGpuSetScanout {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_SET_SCANOUT),
            r: VirtioGpuRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            scanout_id,
            resource_id,
        };

        let resp: VirtioGpuCtrlHdr = self.send_command(&cmd)?;

        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            error!("Failed to set scanout");
            return Err(Errno::EIO);
        }

        // Update scanout state.
        let mut scanouts = self.scanouts.lock();
        if let Some(scanout) = scanouts.iter_mut().find(|s| s.id == scanout_id) {
            scanout.current_resource = Some(resource_id);
            scanout.width = width;
            scanout.height = height;
        }

        Ok(())
    }

    pub fn transfer_to_host_2d(
        &self,
        resource_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> EResult<()> {
        let cmd = VirtioGpuTransferToHost2d {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D),
            r: VirtioGpuRect {
                x,
                y,
                width,
                height,
            },
            offset: 0,
            resource_id,
            padding: 0,
        };

        let resp: VirtioGpuCtrlHdr = self.send_command(&cmd)?;

        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            error!("Failed to transfer to host");
            return Err(Errno::EIO);
        }

        Ok(())
    }

    pub fn flush_resource(&self, resource_id: u32, width: u32, height: u32) -> EResult<()> {
        let cmd = VirtioGpuResourceFlush {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: VirtioGpuRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            resource_id,
            padding: 0,
        };

        let resp: VirtioGpuCtrlHdr = self.send_command(&cmd)?;

        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            error!("Failed to flush resource");
            return Err(Errno::EIO);
        }

        Ok(())
    }

    pub fn initialize_drm_objects(&self) -> EResult<()> {
        let scanouts = self.scanouts.lock();

        // Create one CRTC per scanout
        let mut crtcs = Vec::new();
        for _ in scanouts.iter() {
            let crtc_id = self.obj_counter.alloc();
            let crtc = Arc::new(Crtc::new(crtc_id));
            crtcs.push(crtc);
        }

        // Create one primary plane per CRTC for atomic modesetting
        let mut all_planes = Vec::new();
        for crtc in crtcs.iter() {
            let plane_id = self.obj_counter.alloc();
            let plane = Arc::new(Plane::new(
                plane_id,
                vec![crtc.clone()],
                1, // DRM_PLANE_TYPE_PRIMARY
                vec![DRM_FORMAT_XRGB8888],
            ));
            all_planes.push(plane);

            // Create a cursor plane per CRTC
            let cursor_plane_id = self.obj_counter.alloc();
            let cursor_plane = Arc::new(Plane::new(
                cursor_plane_id,
                vec![crtc.clone()],
                DRM_PLANE_TYPE_CURSOR,
                vec![DRM_FORMAT_ARGB8888],
            ));
            all_planes.push(cursor_plane);
        }

        // Create connectors and encoders
        let mut all_encoders = Vec::new();
        let mut all_connectors = Vec::new();

        for (idx, scanout) in scanouts.iter().enumerate() {
            let crtc = crtcs[idx].clone();

            // Create encoder for this scanout
            let encoder_id = self.obj_counter.alloc();
            let encoder = Arc::new(Encoder::new(encoder_id, vec![crtc.clone()], crtc.clone()));
            all_encoders.push(encoder.clone());

            let preferred = synthesize_preferred_mode(scanout.width, scanout.height);
            let mut modes = vec![preferred];
            modes.extend(
                DMT_MODES
                    .iter()
                    .filter(|m| {
                        m.hdisplay != preferred.hdisplay || m.vdisplay != preferred.vdisplay
                    })
                    .cloned(),
            );

            // Create connector
            let connector_id = self.obj_counter.alloc();
            let connector = Arc::new(Connector::new(
                connector_id,
                drm_mode_connector_state::Connected,
                modes,
                vec![encoder.clone()],
                drm_mode_connector_type::Virtual,
                self.state
                    .next_connector_type_id(drm_mode_connector_type::Virtual),
            ));
            all_connectors.push(connector);
        }

        // Store objects in device
        self.state.crtcs.lock().extend(crtcs);
        self.state.encoders.lock().extend(all_encoders);
        self.state.connectors.lock().extend(all_connectors);
        self.state.planes.lock().extend(all_planes);

        Ok(())
    }

    fn send_cursor_command(&self, cmd: &VirtioGpuUpdateCursor) -> EResult<()> {
        let cmd_buffer = OwnedPhysPages::new(1, AllocFlags::empty())?;
        let cmd_ptr = cmd_buffer.as_hhdm::<VirtioGpuUpdateCursor>();
        unsafe {
            core::ptr::write_volatile(cmd_ptr, *cmd);
        }

        let buffers = vec![(
            cmd_buffer.phys(),
            core::mem::size_of::<VirtioGpuUpdateCursor>(),
            false,
        )];

        {
            let mut queue = self.cursor_queue.lock();
            queue.add_buffer(&buffers)?;
            self.virtio.lock().notify_queue(&queue);
        }

        // Wait for completion
        loop {
            let mut queue = self.cursor_queue.lock();
            if let Some((desc_id, _)) = queue.get_used() {
                queue.release_used_chain(desc_id);
                break;
            }
            drop(queue);
            core::hint::spin_loop();
        }

        Ok(())
    }
}

impl Device for VirtioGpuDevice {
    fn state(&self) -> &DeviceState {
        &self.state
    }

    fn driver_version(&self) -> (i32, i32, i32) {
        (1, 0, 0)
    }

    fn driver_info(&self) -> (&str, &str, &str) {
        ("virtio-gpu", "VirtIO GPU Driver", "2026")
    }

    fn create_dumb(
        &self,
        _file: &DrmFile,
        width: u32,
        height: u32,
        bpp: u32,
    ) -> EResult<(Arc<dyn BufferObject>, u32)> {
        log!("Creating dumb buffer {}x{} @ {}bpp", width, height, bpp);
        let page_size = arch::virt::get_page_size();
        let bytes_per_pixel = bpp.div_ceil(8);
        let pitch = width * bytes_per_pixel;
        let size = (pitch * height) as usize;

        let format = match bpp {
            32 => VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM,
            _ => return Err(Errno::EINVAL),
        };

        let num_pages = size.div_ceil(page_size);
        log!("Allocating {} pages for buffer (size={})", num_pages, size);
        let allocation = OwnedPhysPages::new(num_pages, AllocFlags::empty())?;
        let base_addr = allocation.phys();

        let resource_id = self.alloc_resource_id();
        log!("Using format {} for resource", format);

        self.create_resource_2d(resource_id, width, height, format)?;

        // Attach backing storage
        let page_addrs: Vec<PhysAddr> = (0..num_pages)
            .map(|i| base_addr + (i * page_size))
            .collect();
        if let Err(e) = self.attach_backing(resource_id, &page_addrs) {
            Self::destroy_resource_raw(&self.virtio, &self.ctrl_queue, resource_id, false);
            return Err(e);
        }

        // Store this as the active resource
        self.active_resource.store(resource_id, Ordering::SeqCst);
        log!("Set active resource to {}", resource_id);

        let base_addr = allocation.into_phys();

        let buffer_id = self.obj_counter.alloc();
        let buffer = Arc::new(VirtioGpuBuffer {
            id: buffer_id,
            resource_id,
            base_addr,
            size,
            width,
            height,
            virtio: self.virtio.clone(),
            ctrl_queue: self.ctrl_queue.clone(),
        });

        log!(
            "Created dumb buffer {} with resource {}",
            buffer_id,
            resource_id
        );
        Ok((buffer, pitch))
    }

    fn create_fb(
        &self,
        _file: &DrmFile,
        buffer: Arc<dyn BufferObject>,
        width: u32,
        height: u32,
        format: u32,
        pitch: u32,
    ) -> EResult<Arc<Framebuffer>> {
        Ok(Arc::new(Framebuffer {
            id: self.obj_counter.alloc(),
            format,
            width,
            height,
            pitch,
            offset: 0,
            buffer,
        }))
    }

    fn commit(&self, state: &AtomicState) {
        // Get the framebuffer from the first CRTC state (we only support one CRTC for now)
        let crtc_state = state.crtc_states.values().next();
        let framebuffer = match crtc_state {
            Some(state) => match &state.framebuffer {
                Some(fb) => fb.clone(),
                None => {
                    log!("No framebuffer set on CRTC");
                    return;
                }
            },
            None => {
                log!("No CRTC state in atomic commit");
                return;
            }
        };

        // Get the buffer object from the framebuffer and downcast to VirtioGpuBuffer
        let buffer = framebuffer.buffer.clone();
        let virtio_buffer = match (buffer.as_ref() as &dyn Any).downcast_ref::<VirtioGpuBuffer>() {
            Some(buf) => buf,
            None => {
                error!("Framebuffer buffer is not a VirtioGpuBuffer!");
                return;
            }
        };

        let resource_id = virtio_buffer.resource_id;

        let (scanout_id, needs_set_scanout) = {
            let scanouts = self.scanouts.lock();
            if let Some(scanout) = scanouts.first() {
                let needs = scanout.current_resource != Some(resource_id)
                    || scanout.width != framebuffer.width
                    || scanout.height != framebuffer.height;
                (scanout.id, needs)
            } else {
                error!("No scanouts available!");
                return;
            }
        };

        let transfer_width = framebuffer.width;
        let transfer_height = framebuffer.height;

        // Upload the new framebuffer contents before rebinding the scanout.
        // Otherwise the host can briefly display stale pixels from this resource.
        if let Ok(()) = self.transfer_to_host_2d(resource_id, 0, 0, transfer_width, transfer_height)
        {
            if needs_set_scanout
                && let Err(e) =
                    self.set_scanout(scanout_id, resource_id, transfer_width, transfer_height)
            {
                error!("Failed to set scanout: {:?}", e);
                return;
            }

            // Flush after the upload/bind sequence so the visible scanout only ever sees
            // the freshly transferred contents.
            if let Err(e) = self.flush_resource(resource_id, transfer_width, transfer_height) {
                error!("Failed to flush resource {}: {:?}", resource_id, e);
            }
        } else {
            error!("Failed to transfer resource {} to host", resource_id);
        }
    }

    fn move_cursor(&self, _crtc_id: u32, x: i32, y: i32) -> EResult<()> {
        let scanout_id = {
            let scanouts = self.scanouts.lock();
            scanouts.first().map(|s| s.id).unwrap_or(0)
        };

        self.cursor_x.store(x as u32, Ordering::SeqCst);
        self.cursor_y.store(y as u32, Ordering::SeqCst);

        let resource_id = self.cursor_resource.load(Ordering::SeqCst);

        let cmd = VirtioGpuUpdateCursor {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: VirtioGpuCursorPos {
                scanout_id,
                x: x as u32,
                y: y as u32,
                padding: 0,
            },
            resource_id,
            hot_x: self.cursor_hot_x.load(Ordering::SeqCst),
            hot_y: self.cursor_hot_y.load(Ordering::SeqCst),
            padding: 0,
        };
        self.send_cursor_command(&cmd)?;
        Ok(())
    }
}

pub struct VirtioGpuBuffer {
    id: u32,
    resource_id: u32,
    base_addr: PhysAddr,
    size: usize,
    width: u32,
    height: u32,
    virtio: Arc<SpinMutex<VirtioDevice>>,
    ctrl_queue: Arc<SpinMutex<VirtQueue>>,
}

impl Drop for VirtioGpuBuffer {
    fn drop(&mut self) {
        VirtioGpuDevice::destroy_resource_raw(
            &self.virtio,
            &self.ctrl_queue,
            self.resource_id,
            true,
        );

        let page_size = arch::virt::get_page_size();
        let pages = self.size.div_ceil(page_size);
        unsafe {
            KernelAlloc::dealloc(self.base_addr, pages);
        }
    }
}

impl zinnia::memory::MemoryObject for VirtioGpuBuffer {
    fn try_get_page(&self, page_index: usize) -> Option<PhysAddr> {
        const PAGE_SIZE: usize = 4096;
        let offset = page_index * PAGE_SIZE;
        if offset < self.size {
            Some(PhysAddr::new(self.base_addr.value() + offset))
        } else {
            None
        }
    }
}

impl BufferObject for VirtioGpuBuffer {
    fn id(&self) -> u32 {
        self.id
    }

    fn size(&self) -> usize {
        self.size
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}
