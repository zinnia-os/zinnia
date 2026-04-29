use crate::spec::*;
use core::any::Any;
use virtio::{VirtQueue, VirtioDevice};
use zinnia::alloc::vec;
use zinnia::device::drm::DeviceState;
use zinnia::device::drm::modes::DMT_MODES;
use zinnia::uapi::drm::drm_mode_connector_type;
use zinnia::{
    alloc::{sync::Arc, vec::Vec},
    arch,
    core::sync::atomic::{AtomicU32, Ordering},
    device::drm::{
        Device, DrmFile, IdAllocator,
        object::{AtomicState, BufferObject, Connector, Crtc, Encoder, Framebuffer, Plane},
    },
    error, log,
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr, VirtAddr},
    posix::errno::{EResult, Errno},
    uapi::drm::{
        DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_PLANE_TYPE_CURSOR, drm_mode_connector_state,
        drm_mode_modeinfo,
    },
    util::mutex::spin::SpinMutex,
};

pub struct VirtioGpuDevice {
    state: DeviceState,
    virtio: SpinMutex<VirtioDevice>,
    ctrl_queue: SpinMutex<VirtQueue>,
    ctrl_notify_off: u16,
    cursor_queue: SpinMutex<VirtQueue>,
    cursor_notify_off: u16,
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

struct PhysPageAllocation {
    base_addr: PhysAddr,
    pages: usize,
}

impl PhysPageAllocation {
    fn new(pages: usize) -> EResult<Self> {
        let base_addr =
            KernelAlloc::alloc(pages, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;
        Ok(Self { base_addr, pages })
    }

    fn phys(&self) -> PhysAddr {
        self.base_addr
    }

    fn as_hhdm<T>(&self) -> *mut T {
        self.base_addr.as_hhdm::<T>()
    }

    fn into_phys(self) -> PhysAddr {
        let phys = self.base_addr;
        core::mem::forget(self);
        phys
    }
}

impl Drop for PhysPageAllocation {
    fn drop(&mut self) {
        unsafe {
            KernelAlloc::dealloc(self.base_addr, self.pages);
        }
    }
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
        ctrl_notify_off: u16,
        cursor_queue: SpinMutex<VirtQueue>,
        cursor_notify_off: u16,
    ) -> EResult<Self> {
        let device = Self {
            state: DeviceState::new(),
            virtio: SpinMutex::new(virtio),
            ctrl_queue,
            ctrl_notify_off,
            cursor_queue,
            cursor_notify_off,
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

    fn send_command<T: Copy, R: Copy>(&self, cmd: &T) -> EResult<R> {
        let cmd_ptr = cmd as *const T as *const u8;
        let cmd_phys = VirtAddr::from(cmd_ptr).as_hhdm().ok_or(Errno::EFAULT)?;

        // Allocate response buffer
        let resp = PhysPageAllocation::new(1)?;
        let resp_ptr = resp.as_hhdm::<R>();

        let buffers = vec![
            (cmd_phys, core::mem::size_of::<T>(), false),
            (resp.phys(), core::mem::size_of::<R>(), true),
        ];

        {
            let mut queue = self.ctrl_queue.lock();
            queue.add_buffer(&buffers)?;
        }

        // Notify device
        self.virtio.lock().notify_queue(self.ctrl_notify_off);

        // Wait for response
        loop {
            let mut queue = self.ctrl_queue.lock();
            if queue.get_used().is_some() {
                break;
            }
            drop(queue);

            core::hint::spin_loop();
        }

        let response = unsafe { core::ptr::read_volatile(resp_ptr) };
        Ok(response)
    }

    fn get_display_info(&self) -> EResult<()> {
        let cmd = VirtioGpuGetDisplayInfo {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_GET_DISPLAY_INFO),
        };

        let resp: VirtioGpuRespDisplayInfo = self.send_command(&cmd)?;

        if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
            error!("Failed to get display info");
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

    pub fn attach_backing(&self, resource_id: u32, pages: &[PhysAddr]) -> EResult<()> {
        let page_size = arch::virt::get_page_size();
        // Allocate command buffer for header + memory entries
        let cmd_size = core::mem::size_of::<VirtioGpuResourceAttachBacking>()
            + pages.len() * core::mem::size_of::<VirtioGpuMemEntry>();
        let cmd_pages = cmd_size.div_ceil(page_size);
        let cmd = PhysPageAllocation::new(cmd_pages)?;
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
        let resp = PhysPageAllocation::new(1)?;
        let resp_ptr = resp.as_hhdm::<VirtioGpuCtrlHdr>();

        let buffers = vec![
            (cmd.phys(), cmd_size, false),
            (resp.phys(), core::mem::size_of::<VirtioGpuCtrlHdr>(), true),
        ];

        let mut queue = self.ctrl_queue.lock();
        queue.add_buffer(&buffers)?;
        drop(queue);

        self.virtio.lock().notify_queue(self.ctrl_notify_off);

        // Wait for response
        loop {
            let mut queue = self.ctrl_queue.lock();
            if let Some((_, _)) = queue.get_used() {
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

        // Update scanout state
        let mut scanouts = self.scanouts.lock();
        if let Some(scanout) = scanouts.iter_mut().find(|s| s.id == scanout_id) {
            scanout.current_resource = Some(resource_id);
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

    pub fn initialize_drm_objects(&self, _file: &DrmFile) -> EResult<()> {
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

            // Look up proper modes from the DMT table
            let modes: Vec<drm_mode_modeinfo> = DMT_MODES
                .iter()
                .filter(|m| {
                    m.hdisplay == scanout.width as u16 && m.vdisplay == scanout.height as u16
                })
                .cloned()
                .collect();

            // Create connector
            let connector_id = self.obj_counter.alloc();
            let connector = Arc::new(Connector::new(
                connector_id,
                drm_mode_connector_state::Connected,
                modes,
                vec![encoder.clone()],
                drm_mode_connector_type::Virtual,
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
        let cmd_ptr = cmd as *const VirtioGpuUpdateCursor as *const u8;
        let cmd_phys = VirtAddr::from(cmd_ptr).as_hhdm().ok_or(Errno::EFAULT)?;

        let buffers = vec![(
            cmd_phys,
            core::mem::size_of::<VirtioGpuUpdateCursor>(),
            false,
        )];

        {
            let mut queue = self.cursor_queue.lock();
            queue.add_buffer(&buffers)?;
        }

        self.virtio.lock().notify_queue(self.cursor_notify_off);

        // Wait for completion
        loop {
            let mut queue = self.cursor_queue.lock();
            if queue.get_used().is_some() {
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

    fn driver_info(&self) -> (&[u8], &[u8], &[u8]) {
        (b"virtio-gpu", b"VirtIO GPU Driver", b"2026")
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

        let num_pages = size.div_ceil(page_size);
        log!("Allocating {} pages for buffer (size={})", num_pages, size);
        let base_addr = PhysPageAllocation::new(num_pages)?.into_phys();

        let resource_id = self.alloc_resource_id();
        let format = match bpp {
            32 => VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM,
            _ => return Err(Errno::EINVAL),
        };
        log!("Using format {} for resource", format);

        self.create_resource_2d(resource_id, width, height, format)?;

        // Attach backing storage
        let page_addrs: Vec<PhysAddr> = (0..num_pages)
            .map(|i| base_addr + (i * page_size))
            .collect();
        self.attach_backing(resource_id, &page_addrs)?;

        // Store this as the active resource
        self.active_resource.store(resource_id, Ordering::SeqCst);
        log!("Set active resource to {}", resource_id);

        let buffer_id = self.obj_counter.alloc();
        let buffer = Arc::new(VirtioGpuBuffer {
            id: buffer_id,
            resource_id,
            base_addr,
            size,
            width,
            height,
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

        // Get scanout information and check whether the bound resource is already what we want.
        let (scanout_id, scanout_width, scanout_height, needs_set_scanout) = {
            let scanouts = self.scanouts.lock();
            if let Some(scanout) = scanouts.first() {
                let needs = scanout.current_resource != Some(resource_id);
                (scanout.id, scanout.width, scanout.height, needs)
            } else {
                error!("No scanouts available!");
                return;
            }
        };

        let transfer_width = framebuffer.width.min(scanout_width);
        let transfer_height = framebuffer.height.min(scanout_height);

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

    fn set_cursor(
        &self,
        _crtc_id: u32,
        buffer: Option<Arc<dyn BufferObject>>,
        width: u32,
        height: u32,
        hot_x: i32,
        hot_y: i32,
    ) -> EResult<()> {
        let scanout_id = {
            let scanouts = self.scanouts.lock();
            scanouts.first().map(|s| s.id).unwrap_or(0)
        };

        // Store hotspot values
        self.cursor_hot_x.store(hot_x as u32, Ordering::SeqCst);
        self.cursor_hot_y.store(hot_y as u32, Ordering::SeqCst);

        match buffer {
            Some(buf) => {
                let virtio_buf = (buf.as_ref() as &dyn Any)
                    .downcast_ref::<VirtioGpuBuffer>()
                    .ok_or(Errno::EINVAL)?;
                let resource_id = virtio_buf.resource_id;

                // Transfer cursor image data to host
                self.transfer_to_host_2d(resource_id, 0, 0, width, height)?;

                self.cursor_resource.store(resource_id, Ordering::SeqCst);

                let cmd = VirtioGpuUpdateCursor {
                    hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_UPDATE_CURSOR),
                    pos: VirtioGpuCursorPos {
                        scanout_id,
                        x: self.cursor_x.load(Ordering::SeqCst),
                        y: self.cursor_y.load(Ordering::SeqCst),
                        padding: 0,
                    },
                    resource_id,
                    hot_x: hot_x as u32,
                    hot_y: hot_y as u32,
                    padding: 0,
                };
                self.send_cursor_command(&cmd)?;
            }
            None => {
                // Hide cursor by setting resource_id to 0
                self.cursor_resource.store(0, Ordering::SeqCst);

                let cmd = VirtioGpuUpdateCursor {
                    hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_UPDATE_CURSOR),
                    pos: VirtioGpuCursorPos {
                        scanout_id,
                        x: 0,
                        y: 0,
                        padding: 0,
                    },
                    resource_id: 0,
                    hot_x: 0,
                    hot_y: 0,
                    padding: 0,
                };
                self.send_cursor_command(&cmd)?;
            }
        }
        Ok(())
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
}

impl Drop for VirtioGpuBuffer {
    fn drop(&mut self) {
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
