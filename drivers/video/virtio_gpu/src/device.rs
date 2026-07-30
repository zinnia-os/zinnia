use crate::{
    command::{
        CapsetInfo, CreateResource2d, CreateResource3d, FlushResource, GetCapset, GetCapsetInfo,
        GetDisplayInfo, Rect, ScanoutMode, SetScanout, Submit3d, TransferHost3d, TransferToHost2d,
        UpdateCursor, decode_capset_info, decode_display_info,
    },
    context::RenderContext,
    dma::DmaRegion,
    error::GpuError,
    queue::{ControlQueue, CursorQueue, Fencing},
    resource::{GpuBuffer, Resource},
    spec,
};
use core::any::Any;
use zinnia::{
    alloc::{sync::Arc, vec, vec::Vec},
    arch,
    device::drm::{
        Device, DeviceState, DrmFile, IdAllocator,
        modes::{DMT_MODES, synthesize_preferred_mode},
        object::{
            AtomicState, BufferObject, Connector, Crtc, Encoder, Framebuffer, ModeObject, Plane,
        },
    },
    error, log,
    memory::{UserPtr, VirtAddr},
    posix::errno::{EResult, Errno},
    uapi::drm::{
        DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_PLANE_TYPE_CURSOR, DRM_PLANE_TYPE_PRIMARY,
        drm_mode_connector_state, drm_mode_connector_type, virtgpu,
    },
    util::mutex::spin::SpinMutex,
};

const MAX_EXECBUFFER_SIZE: usize = 1 << 20;
const DUMB_BITS_PER_PIXEL: u32 = 32;

struct Scanout {
    id: u32,
    width: u32,
    height: u32,
    resource_id: Option<u32>,
}

#[derive(Default)]
struct CursorState {
    resource: Option<Arc<Resource>>,
    scanout_id: u32,
    x: i32,
    y: i32,
    hot_x: u32,
    hot_y: u32,
}

pub struct GpuFile {
    context: SpinMutex<Option<Arc<RenderContext>>>,
}

impl GpuFile {
    fn new() -> Self {
        Self {
            context: SpinMutex::new(None),
        }
    }
}

pub struct VirtioGpuDevice {
    state: DeviceState,
    ctrl: Arc<ControlQueue>,
    cursor: Arc<CursorQueue>,
    scanouts: SpinMutex<Vec<Scanout>>,
    cursor_state: SpinMutex<CursorState>,
    capsets: Vec<CapsetInfo>,
    accelerated: bool,
    resource_ids: IdAllocator,
    context_ids: IdAllocator,
    obj_counter: IdAllocator,
}

impl VirtioGpuDevice {
    pub fn new(
        ctrl: Arc<ControlQueue>,
        cursor: Arc<CursorQueue>,
        accelerated: bool,
        num_capsets: u32,
    ) -> Result<Self, GpuError> {
        let modes = Self::query_display_info(&ctrl)?;
        let capsets = Self::query_capsets(&ctrl, num_capsets);

        let scanouts: Vec<_> = modes
            .into_iter()
            .map(|mode| Scanout {
                id: mode.id,
                width: mode.width,
                height: mode.height,
                resource_id: None,
            })
            .collect();

        let obj_counter = IdAllocator::new();
        let state = DeviceState::new();

        // Init DRM objects.
        {
            let mut crtcs = Vec::new();
            let mut planes = Vec::new();
            let mut encoders = Vec::new();
            let mut connectors = Vec::new();

            for scanout in scanouts.iter() {
                let crtc = Arc::new(Crtc::new(obj_counter.alloc()));

                planes.push(Arc::new(Plane::new(
                    obj_counter.alloc(),
                    vec![crtc.clone()],
                    DRM_PLANE_TYPE_PRIMARY,
                    vec![DRM_FORMAT_XRGB8888],
                )));
                planes.push(Arc::new(Plane::new(
                    obj_counter.alloc(),
                    vec![crtc.clone()],
                    DRM_PLANE_TYPE_CURSOR,
                    vec![DRM_FORMAT_ARGB8888],
                )));

                let encoder = Arc::new(Encoder::new(
                    obj_counter.alloc(),
                    vec![crtc.clone()],
                    crtc.clone(),
                ));
                encoders.push(encoder.clone());

                let preferred = synthesize_preferred_mode(scanout.width, scanout.height);
                let mut modes = vec![preferred];
                modes.extend(
                    DMT_MODES
                        .iter()
                        .filter(|mode| {
                            mode.hdisplay != preferred.hdisplay
                                || mode.vdisplay != preferred.vdisplay
                        })
                        .cloned(),
                );

                connectors.push(Arc::new(Connector::new(
                    obj_counter.alloc(),
                    drm_mode_connector_state::Connected,
                    modes,
                    vec![encoder],
                    drm_mode_connector_type::Virtual,
                    state.next_connector_type_id(drm_mode_connector_type::Virtual),
                )));

                crtcs.push(crtc);
            }

            state.crtcs.lock().extend(crtcs);
            state.encoders.lock().extend(encoders);
            state.connectors.lock().extend(connectors);
            state.planes.lock().extend(planes);
        }

        Ok(Self {
            state,
            ctrl,
            cursor,
            scanouts: SpinMutex::new(scanouts),
            cursor_state: SpinMutex::new(CursorState::default()),
            capsets,
            accelerated,
            resource_ids: IdAllocator::new(),
            context_ids: IdAllocator::new(),
            obj_counter,
        })
    }

    fn query_display_info(ctrl: &ControlQueue) -> Result<Vec<ScanoutMode>, GpuError> {
        let mut response = vec![0u8; spec::resp_display_info::SIZE];
        ctrl.execute(0, &GetDisplayInfo, Fencing::Unfenced, &mut response)?;
        decode_display_info(&response)
    }

    fn query_capsets(ctrl: &ControlQueue, num_capsets: u32) -> Vec<CapsetInfo> {
        let mut capsets = Vec::new();

        for index in 0..num_capsets {
            let mut response = vec![0u8; spec::resp_capset_info::SIZE];
            let command = GetCapsetInfo { index };
            if ctrl
                .execute(0, &command, Fencing::Unfenced, &mut response)
                .is_err()
            {
                continue;
            }

            match decode_capset_info(&response) {
                Ok(info) => {
                    log!(
                        "Capset {} version {} ({} bytes)",
                        info.id,
                        info.max_version,
                        info.max_size
                    );
                    capsets.push(info);
                }
                Err(error) => error!("Failed to decode capset {index}: {error}"),
            }
        }

        capsets
    }

    fn scanout_for_crtc(&self, crtc_id: u32) -> Option<u32> {
        let index = self
            .state
            .crtcs
            .lock()
            .iter()
            .position(|crtc| crtc.id() == crtc_id)?;
        self.scanouts.lock().get(index).map(|scanout| scanout.id)
    }

    fn buffer_of(framebuffer: &Framebuffer) -> Result<&GpuBuffer, GpuError> {
        (framebuffer.buffer.as_ref() as &dyn Any)
            .downcast_ref::<GpuBuffer>()
            .ok_or(GpuError::UnknownResource)
    }

    fn gpu_buffer(buffer: &Arc<dyn BufferObject>) -> Result<&GpuBuffer, GpuError> {
        (buffer.as_ref() as &dyn Any)
            .downcast_ref::<GpuBuffer>()
            .ok_or(GpuError::UnknownResource)
    }

    fn present(&self, crtc_id: u32, framebuffer: &Framebuffer) -> Result<(), GpuError> {
        let scanout_id = self
            .scanout_for_crtc(crtc_id)
            .ok_or(GpuError::InvalidScanoutId)?;
        let buffer = Self::buffer_of(framebuffer)?;
        let resource_id = buffer.resource().id();
        let area = Rect::sized(framebuffer.width, framebuffer.height);

        if buffer.is_dumb() {
            self.ctrl.execute_checked(
                0,
                &TransferToHost2d {
                    resource_id,
                    rect: area,
                    offset: 0,
                },
                Fencing::Unfenced,
            )?;
        }

        let rebind = {
            let scanouts = self.scanouts.lock();
            scanouts
                .iter()
                .find(|scanout| scanout.id == scanout_id)
                .is_none_or(|scanout| {
                    scanout.resource_id != Some(resource_id)
                        || scanout.width != framebuffer.width
                        || scanout.height != framebuffer.height
                })
        };

        if rebind {
            self.ctrl.execute_checked(
                0,
                &SetScanout {
                    scanout_id,
                    resource_id,
                    rect: area,
                },
                Fencing::Unfenced,
            )?;

            let mut scanouts = self.scanouts.lock();
            if let Some(scanout) = scanouts.iter_mut().find(|scanout| scanout.id == scanout_id) {
                scanout.resource_id = Some(resource_id);
                scanout.width = framebuffer.width;
                scanout.height = framebuffer.height;
            }
        }

        self.ctrl.execute_checked(
            0,
            &FlushResource {
                resource_id,
                rect: area,
            },
            Fencing::Unfenced,
        )?;

        Ok(())
    }

    fn file_state<'a>(&self, file: &'a DrmFile) -> EResult<&'a GpuFile> {
        file.driver_private::<GpuFile>().ok_or(Errno::ENOTTY)
    }

    fn render_context(&self, file: &DrmFile) -> Result<Arc<RenderContext>, GpuError> {
        let gpu_file = self
            .file_state(file)
            .map_err(|_| GpuError::NoRenderContext)?;

        if let Some(context) = gpu_file.context.lock().clone() {
            return Ok(context);
        }

        if !self.accelerated {
            return Err(GpuError::NotAccelerated);
        }

        let context = Arc::new(RenderContext::create(
            self.ctrl.clone(),
            self.context_ids.alloc(),
            "zinnia",
        )?);

        Ok(gpu_file.context.lock().get_or_insert(context).clone())
    }

    fn capset(&self, id: u32, version: u32) -> Result<CapsetInfo, GpuError> {
        self.capsets
            .iter()
            .find(|info| info.id == id && info.max_version >= version)
            .copied()
            .ok_or(GpuError::InvalidParameter)
    }

    fn resource_by_handle(&self, file: &DrmFile, handle: u32) -> EResult<Arc<Resource>> {
        let buffer = file.get_buffer(handle)?;
        Ok(Self::gpu_buffer(&buffer)?.resource().clone())
    }

    fn ioctl_getparam(&self, arg: VirtAddr) -> EResult<()> {
        let ptr = UserPtr::<virtgpu::drm_virtgpu_getparam>::new(arg);
        let val = ptr.read().ok_or(Errno::EFAULT)?;

        let value: u32 = match val.param {
            virtgpu::VIRTGPU_PARAM_3D_FEATURES => self.accelerated as u32,
            virtgpu::VIRTGPU_PARAM_CAPSET_QUERY_FIX => 1,
            _ => 0,
        };

        UserPtr::<u32>::new(VirtAddr::new(val.value as usize))
            .write(value)
            .ok_or(Errno::EFAULT)
    }

    fn ioctl_get_caps(&self, arg: VirtAddr) -> EResult<()> {
        let ptr = UserPtr::<virtgpu::drm_virtgpu_get_caps>::new(arg);
        let val = ptr.read().ok_or(Errno::EFAULT)?;

        let info = self.capset(val.cap_set_id, val.cap_set_ver)?;
        let command = GetCapset {
            capset_id: val.cap_set_id,
            version: val.cap_set_ver,
            max_size: info.max_size as usize,
        };

        let mut response = vec![0u8; spec::resp_capset::DATA + info.max_size as usize];
        self.ctrl
            .execute(0, &command, Fencing::Unfenced, &mut response)?;

        let wanted = (val.size as usize).min(info.max_size as usize);
        let payload = response
            .get(spec::resp_capset::DATA..spec::resp_capset::DATA + wanted)
            .ok_or(Errno::EIO)?;

        UserPtr::<u8>::new(VirtAddr::new(val.addr as usize))
            .write_slice(payload)
            .ok_or(Errno::EFAULT)
    }

    fn ioctl_resource_create(&self, file: &DrmFile, arg: VirtAddr) -> EResult<()> {
        let mut ptr = UserPtr::<virtgpu::drm_virtgpu_resource_create>::new(arg);
        let mut val = ptr.read().ok_or(Errno::EFAULT)?;

        let context = self.render_context(file)?;
        let page_size = arch::virt::get_page_size();
        let size = (val.size as usize)
            .max(page_size)
            .next_multiple_of(page_size);
        let backing = Some(DmaRegion::new(size)?);

        let resource_id = self.resource_ids.alloc();
        self.ctrl.execute_checked(
            context.id(),
            &CreateResource3d {
                resource_id,
                target: val.target,
                format: val.format,
                bind: val.bind,
                width: val.width,
                height: val.height,
                depth: val.depth,
                array_size: val.array_size,
                last_level: val.last_level,
                nr_samples: val.nr_samples,
                flags: val.flags,
            },
            Fencing::Unfenced,
        )?;

        let resource = Resource::adopt(self.ctrl.clone(), resource_id, backing)?;
        context.attach(resource_id)?;

        let handle = self.obj_counter.alloc();
        let buffer: Arc<dyn BufferObject> = Arc::new(GpuBuffer::new(
            handle, resource, val.width, val.height, false,
        ));
        file.insert_buffer(buffer)?;

        val.bo_handle = handle;
        val.res_handle = resource_id;
        ptr.write(val).ok_or(Errno::EFAULT)
    }

    fn ioctl_resource_info(&self, file: &DrmFile, arg: VirtAddr) -> EResult<()> {
        let mut ptr = UserPtr::<virtgpu::drm_virtgpu_resource_info>::new(arg);
        let mut val = ptr.read().ok_or(Errno::EFAULT)?;

        let resource = self.resource_by_handle(file, val.bo_handle)?;
        val.res_handle = resource.id();
        val.size = resource.size() as u32;
        val.blob_mem = 0;

        ptr.write(val).ok_or(Errno::EFAULT)
    }

    fn ioctl_map(&self, file: &DrmFile, arg: VirtAddr) -> EResult<()> {
        let mut ptr = UserPtr::<virtgpu::drm_virtgpu_map>::new(arg);
        let mut val = ptr.read().ok_or(Errno::EFAULT)?;

        let buffer = file.get_buffer(val.handle)?;
        val.offset = (buffer.id() as u64) << 32;

        ptr.write(val).ok_or(Errno::EFAULT)
    }

    fn ioctl_transfer(&self, file: &DrmFile, arg: VirtAddr, to_host: bool) -> EResult<()> {
        let ptr = UserPtr::<virtgpu::drm_virtgpu_3d_transfer_to_host>::new(arg);
        let val = ptr.read().ok_or(Errno::EFAULT)?;

        let resource = self.resource_by_handle(file, val.bo_handle)?;
        let context = self.render_context(file)?;

        let fence = self.ctrl.execute_checked(
            context.id(),
            &TransferHost3d {
                to_host,
                resource_id: resource.id(),
                area: crate::command::Box3d {
                    x: val.r#box.x,
                    y: val.r#box.y,
                    z: val.r#box.z,
                    w: val.r#box.w,
                    h: val.r#box.h,
                    d: val.r#box.d,
                },
                offset: val.offset as u64,
                level: val.level,
                stride: val.stride,
                layer_stride: val.layer_stride,
            },
            Fencing::Fenced,
        )?;

        resource.record_fence(fence);
        if !to_host {
            self.ctrl.wait_fence(fence)?;
        }

        Ok(())
    }

    fn ioctl_execbuffer(&self, file: &DrmFile, arg: VirtAddr) -> EResult<()> {
        let mut ptr = UserPtr::<virtgpu::drm_virtgpu_execbuffer>::new(arg);
        let mut val = ptr.read().ok_or(Errno::EFAULT)?;

        let unsupported = virtgpu::VIRTGPU_EXECBUF_FENCE_FD_IN
            | virtgpu::VIRTGPU_EXECBUF_FENCE_FD_OUT
            | virtgpu::VIRTGPU_EXECBUF_RING_IDX;
        if val.flags & unsupported != 0 || val.num_in_syncobjs > 0 || val.num_out_syncobjs > 0 {
            return Err(Errno::EINVAL);
        }
        if val.size == 0 || val.size as usize > MAX_EXECBUFFER_SIZE {
            return Err(Errno::EINVAL);
        }

        let mut stream = DmaRegion::new(val.size as usize)?;
        UserPtr::<u8>::new(VirtAddr::new(val.command as usize))
            .read_slice(stream.as_mut_slice())
            .ok_or(Errno::EFAULT)?;

        let mut resources = Vec::with_capacity(val.num_bo_handles as usize);
        if val.num_bo_handles > 0 {
            let mut handles = vec![0u32; val.num_bo_handles as usize];
            UserPtr::<u32>::new(VirtAddr::new(val.bo_handles as usize))
                .read_slice(&mut handles)
                .ok_or(Errno::EFAULT)?;

            for handle in handles {
                resources.push(self.resource_by_handle(file, handle)?);
            }
        }

        let context = self.render_context(file)?;
        let fence =
            self.ctrl
                .execute_checked(context.id(), &Submit3d::new(stream), Fencing::Fenced)?;

        for resource in resources {
            resource.record_fence(fence);
        }

        val.fence_fd = -1;
        ptr.write(val).ok_or(Errno::EFAULT)
    }

    fn ioctl_wait(&self, file: &DrmFile, arg: VirtAddr) -> EResult<()> {
        let ptr = UserPtr::<virtgpu::drm_virtgpu_3d_wait>::new(arg);
        let val = ptr.read().ok_or(Errno::EFAULT)?;

        let resource = self.resource_by_handle(file, val.handle)?;

        if val.flags & virtgpu::VIRTGPU_WAIT_NOWAIT != 0 {
            self.ctrl.drain();
            return if resource.is_busy() {
                Err(Errno::EBUSY)
            } else {
                Ok(())
            };
        }

        resource.wait_idle()?;
        Ok(())
    }

    fn send_cursor(&self, move_only: bool) -> Result<(), GpuError> {
        let state = self.cursor_state.lock();
        let command = UpdateCursor {
            move_only,
            scanout_id: state.scanout_id,
            resource_id: state.resource.as_ref().map_or(0, |x| x.id()),
            x: state.x,
            y: state.y,
            hot_x: state.hot_x,
            hot_y: state.hot_y,
        };
        drop(state);

        self.cursor.submit(&command)
    }
}

impl Device for VirtioGpuDevice {
    fn state(&self) -> &DeviceState {
        &self.state
    }

    fn driver_version(&self) -> (i32, i32, i32) {
        (0, 0, 0)
    }

    fn driver_info(&self) -> (&str, &str, &str) {
        ("virtio_gpu", "virtio GPU", "0")
    }

    fn open_file(&self) -> EResult<Option<Arc<dyn Any + Send + Sync>>> {
        Ok(Some(Arc::new(GpuFile::new())))
    }

    fn import_buffer(&self, file: &DrmFile, buffer: &Arc<dyn BufferObject>) -> EResult<()> {
        if !self.accelerated {
            return Ok(());
        }

        let Ok(gpu_buffer) = Self::gpu_buffer(buffer) else {
            return Ok(());
        };
        if gpu_buffer.is_dumb() {
            return Ok(());
        }

        let Ok(gpu_file) = self.file_state(file) else {
            return Ok(());
        };

        let context = gpu_file.context.lock().clone();
        let Some(context) = context else {
            return Ok(());
        };

        context.attach(gpu_buffer.resource().id())?;
        Ok(())
    }

    fn create_dumb(
        &self,
        _file: &DrmFile,
        width: u32,
        height: u32,
        bpp: u32,
    ) -> EResult<(Arc<dyn BufferObject>, u32)> {
        if bpp != DUMB_BITS_PER_PIXEL {
            return Err(Errno::EINVAL);
        }

        let pitch = width * (bpp / 8);
        let page_size = arch::virt::get_page_size();
        let size = (pitch as usize * height as usize).next_multiple_of(page_size);
        let backing = DmaRegion::new(size)?;

        let resource_id = self.resource_ids.alloc();
        self.ctrl.execute_checked(
            0,
            &CreateResource2d {
                resource_id,
                format: spec::format::B8G8R8X8_UNORM,
                width,
                height,
            },
            Fencing::Unfenced,
        )?;

        let resource = Resource::adopt(self.ctrl.clone(), resource_id, Some(backing))?;
        let handle = self.obj_counter.alloc();

        Ok((
            Arc::new(GpuBuffer::new(handle, resource, width, height, true)),
            pitch,
        ))
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
        for (crtc_id, crtc_state) in state.crtc_states.iter() {
            let Some(framebuffer) = crtc_state.framebuffer.as_ref() else {
                continue;
            };

            if let Err(error) = self.present(*crtc_id, framebuffer) {
                error!("Failed to present on CRTC {crtc_id}: {error}");
            }
        }
    }

    fn set_cursor(
        &self,
        crtc_id: u32,
        buffer: Option<Arc<dyn BufferObject>>,
        width: u32,
        height: u32,
        hot_x: i32,
        hot_y: i32,
    ) -> EResult<()> {
        let scanout_id = self.scanout_for_crtc(crtc_id);

        let Some(buffer) = buffer else {
            let previous = self.cursor_state.lock().resource.take();
            drop(previous);
            self.send_cursor(false)?;
            return Ok(());
        };

        let resource = Self::gpu_buffer(&buffer)?.resource().clone();

        self.ctrl.execute_checked(
            0,
            &TransferToHost2d {
                resource_id: resource.id(),
                rect: Rect::sized(width, height),
                offset: 0,
            },
            Fencing::Unfenced,
        )?;

        let previous = {
            let mut state = self.cursor_state.lock();
            state.scanout_id = scanout_id.unwrap_or(state.scanout_id);
            state.hot_x = hot_x.max(0) as u32;
            state.hot_y = hot_y.max(0) as u32;
            state.resource.replace(resource)
        };
        drop(previous);

        self.send_cursor(false)?;
        Ok(())
    }

    fn move_cursor(&self, crtc_id: u32, x: i32, y: i32) -> EResult<()> {
        let scanout_id = self.scanout_for_crtc(crtc_id);

        {
            let mut state = self.cursor_state.lock();
            state.scanout_id = scanout_id.unwrap_or(state.scanout_id);
            state.x = x;
            state.y = y;
        }

        self.send_cursor(true)?;
        Ok(())
    }

    fn driver_ioctl(&self, file: &DrmFile, request: u32, arg: VirtAddr) -> EResult<usize> {
        match request {
            virtgpu::DRM_IOCTL_VIRTGPU_GETPARAM => self.ioctl_getparam(arg)?,
            virtgpu::DRM_IOCTL_VIRTGPU_GET_CAPS => self.ioctl_get_caps(arg)?,
            virtgpu::DRM_IOCTL_VIRTGPU_RESOURCE_CREATE => self.ioctl_resource_create(file, arg)?,
            virtgpu::DRM_IOCTL_VIRTGPU_RESOURCE_INFO => self.ioctl_resource_info(file, arg)?,
            virtgpu::DRM_IOCTL_VIRTGPU_MAP => self.ioctl_map(file, arg)?,
            virtgpu::DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST => self.ioctl_transfer(file, arg, true)?,
            virtgpu::DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST => {
                self.ioctl_transfer(file, arg, false)?
            }
            virtgpu::DRM_IOCTL_VIRTGPU_EXECBUFFER => self.ioctl_execbuffer(file, arg)?,
            virtgpu::DRM_IOCTL_VIRTGPU_WAIT => self.ioctl_wait(file, arg)?,
            _ => return Err(Errno::ENOTTY),
        }

        Ok(0)
    }
}
