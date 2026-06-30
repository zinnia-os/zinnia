use super::{
    DeviceState,
    object::{AtomicState, BufferObject, Connector, Crtc, Encoder, Plane},
};
use crate::{
    arch,
    boot::BootInfo,
    device::drm::{
        Device, DrmFile, IdAllocator,
        modes::{DMT_MODES, synthesize_preferred_mode},
        object::Framebuffer,
    },
    memory::{
        MemoryObject, MmioView, PhysAddr,
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
        virt::VmCacheType,
    },
    posix::errno::{EResult, Errno},
    uapi::drm::{
        DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_PLANE_TYPE_CURSOR, DRM_PLANE_TYPE_PRIMARY,
        drm_mode_connector_state, drm_mode_connector_type,
    },
};
use alloc::{sync::Arc, vec};
use core::any::Any;

struct PlainDevice {
    state: DeviceState,
    width: u32,
    height: u32,
    bpp: u32,
    stride: u32,
    addr: MmioView, // Shared DRM object storage (device-global)
    obj_counter: IdAllocator,
}

impl Device for PlainDevice {
    fn state(&self) -> &DeviceState {
        &self.state
    }

    fn driver_version(&self) -> (i32, i32, i32) {
        (0, 1, 0)
    }

    fn driver_info(&self) -> (&str, &str, &str) {
        ("plainfb", "Plain Framebuffer", "0")
    }

    fn create_dumb(
        &self,
        _file: &DrmFile,
        width: u32,
        height: u32,
        bpp: u32,
    ) -> EResult<(Arc<dyn BufferObject>, u32)> {
        if bpp != 32 || width == 0 || height == 0 {
            return Err(Errno::EINVAL);
        }
        if width > 16384 || height > 16384 {
            return Err(Errno::EINVAL);
        }

        let bytes_per_pixel = bpp.div_ceil(8);
        let pitch = width * bytes_per_pixel;
        let size = (pitch * height) as usize;

        // Allocate physical memory for the buffer
        let buffer_addr =
            KernelAlloc::alloc_bytes(size, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;

        Ok((
            Arc::new(PlainDumbBuffer {
                id: self.obj_counter.alloc(),
                size,
                width,
                height,
                addr: buffer_addr,
            }),
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

    fn set_cursor(
        &self,
        crtc_id: u32,
        buffer: Option<Arc<dyn BufferObject>>,
        width: u32,
        height: u32,
        hot_x: i32,
        hot_y: i32,
    ) -> EResult<()> {
        let fb = match buffer {
            Some(buffer) => Some(Arc::new(Framebuffer {
                id: self.obj_counter.alloc(),
                format: DRM_FORMAT_ARGB8888,
                width,
                height,
                pitch: width.checked_mul(4).ok_or(Errno::EINVAL)?,
                offset: 0,
                buffer,
            })),
            None => None,
        };
        if let Some(plane) = self.cursor_plane() {
            let mut state = plane.state.lock();
            state.crtc_id = crtc_id;
            state.crtc_w = width;
            state.crtc_h = height;
            state.src_x = 0;
            state.src_y = 0;
            state.src_w = width << 16;
            state.src_h = height << 16;
            state.hot_x = hot_x;
            state.hot_y = hot_y;
            state.fb = fb;
        }
        self.redraw();
        Ok(())
    }

    fn move_cursor(&self, crtc_id: u32, x: i32, y: i32) -> EResult<()> {
        if let Some(plane) = self.cursor_plane() {
            let mut state = plane.state.lock();
            state.crtc_id = crtc_id;
            state.crtc_x = x;
            state.crtc_y = y;
        }
        self.redraw();
        Ok(())
    }

    fn commit(&self, _state: &AtomicState) {
        self.redraw();
    }
}

fn blend_over(src: u32, dst: u32, alpha: u32) -> u32 {
    let inv = 255 - alpha;
    let chan = |shift: u32| {
        let s = (src >> shift) & 0xff;
        let d = (dst >> shift) & 0xff;
        ((s * alpha + d * inv) / 255) & 0xff
    };
    0xff00_0000 | (chan(16) << 16) | (chan(8) << 8) | chan(0)
}

impl PlainDevice {
    fn cursor_plane(&self) -> Option<Arc<Plane>> {
        self.state
            .planes
            .lock()
            .iter()
            .find(|p| p.plane_type == DRM_PLANE_TYPE_CURSOR)
            .cloned()
    }

    fn redraw(&self) {
        let (primary, cursor) = {
            let planes = self.state.planes.lock();
            let primary = planes
                .iter()
                .find(|p| p.plane_type == DRM_PLANE_TYPE_PRIMARY)
                .and_then(|p| p.state.lock().fb.clone());
            let cursor = planes
                .iter()
                .find(|p| p.plane_type == DRM_PLANE_TYPE_CURSOR)
                .and_then(|p| {
                    let s = p.state.lock();
                    s.fb.clone()
                        .map(|fb| (fb, s.crtc_x - s.hot_x, s.crtc_y - s.hot_y))
                });
            (primary, cursor)
        };

        if let Some(fb) = primary {
            self.blit_primary(&fb);
        }
        if let Some((fb, x, y)) = cursor {
            self.blend_cursor(&fb, x, y);
        }
    }

    fn blit_primary(&self, framebuffer: &Framebuffer) {
        let Some(buffer) =
            (framebuffer.buffer.as_ref() as &dyn Any).downcast_ref::<PlainDumbBuffer>()
        else {
            return;
        };
        let src_base = buffer.addr.as_hhdm::<u8>();
        let dst_base = self.addr.base() as *mut u8;
        let src_stride = framebuffer.pitch as usize;
        let dst_stride = self.stride as usize;
        let bpp_bytes = (self.bpp.div_ceil(8)) as usize;
        let copy_w = (framebuffer.width as usize).min(self.width as usize) * bpp_bytes;
        let copy_h = (framebuffer.height as usize).min(self.height as usize);

        for y in 0..copy_h {
            let src_off = y * src_stride + framebuffer.offset as usize;
            let dst_off = y * dst_stride;
            if src_off + copy_w > buffer.size || dst_off + copy_w > self.addr.len() {
                break;
            }
            // SAFETY: both ranges are bounds-checked against the buffer and the
            // scanout region above, and the regions don't overlap.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src_base.add(src_off),
                    dst_base.add(dst_off),
                    copy_w,
                );
            }
        }
    }

    fn blend_cursor(&self, framebuffer: &Framebuffer, pos_x: i32, pos_y: i32) {
        let Some(buffer) =
            (framebuffer.buffer.as_ref() as &dyn Any).downcast_ref::<PlainDumbBuffer>()
        else {
            return;
        };
        if self.bpp != 32 {
            return;
        }
        let src_base = buffer.addr.as_hhdm::<u8>();
        let dst_base = self.addr.base() as *mut u8;
        let src_stride = framebuffer.pitch as usize;
        let dst_stride = self.stride as usize;
        let cur_w = framebuffer.width as i32;
        let cur_h = framebuffer.height as i32;
        let disp_w = self.width as i32;
        let disp_h = self.height as i32;

        for row in 0..cur_h {
            let dy = pos_y + row;
            if dy < 0 || dy >= disp_h {
                continue;
            }
            for col in 0..cur_w {
                let dx = pos_x + col;
                if dx < 0 || dx >= disp_w {
                    continue;
                }
                let src_off = row as usize * src_stride + col as usize * 4;
                let dst_off = dy as usize * dst_stride + dx as usize * 4;
                if src_off + 4 > buffer.size || dst_off + 4 > self.addr.len() {
                    continue;
                }
                // SAFETY: both offsets are bounds-checked against the cursor buffer
                // and the scanout region just above.
                unsafe {
                    let src = (src_base.add(src_off) as *const u32).read_unaligned();
                    let alpha = (src >> 24) & 0xff;
                    if alpha == 0 {
                        continue;
                    }
                    let dst_ptr = dst_base.add(dst_off) as *mut u32;
                    let out = if alpha == 255 {
                        src | 0xff00_0000
                    } else {
                        blend_over(src, dst_ptr.read_unaligned(), alpha)
                    };
                    dst_ptr.write_unaligned(out);
                }
            }
        }
    }
}

struct PlainDumbBuffer {
    id: u32,
    width: u32,
    size: usize,
    height: u32,
    addr: PhysAddr,
}

impl Drop for PlainDumbBuffer {
    fn drop(&mut self) {
        let pages = self.size.div_ceil(arch::virt::get_page_size());
        unsafe {
            KernelAlloc::dealloc(self.addr, pages);
        }
    }
}

impl BufferObject for PlainDumbBuffer {
    fn size(&self) -> usize {
        self.size
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn id(&self) -> u32 {
        self.id
    }
}

impl MemoryObject for PlainDumbBuffer {
    fn try_get_page(&self, page_index: usize) -> Option<PhysAddr> {
        let page_size = arch::virt::get_page_size();
        let offset = page_index * page_size;
        if offset < self.size {
            Some(self.addr + offset)
        } else {
            None
        }
    }
}

#[initgraph::task(
    name = "generic.drm.plainfb",
    depends = [crate::vfs::VFS_DEV_MOUNT_STAGE, crate::process::PROCESS_STAGE]
)]
fn PLAINFB_STAGE() {
    if !BootInfo::get()
        .command_line
        .get_bool("plainfb")
        .unwrap_or(false)
    {
        return;
    }

    let Some(fb) = &BootInfo::get().framebuffer else {
        warn!("No framebuffer passed, not creating a plainfb card!");
        return;
    };

    // Create the shared device with empty object storage
    let device = Arc::new(PlainDevice {
        state: DeviceState::new(),
        width: fb.width as _,
        height: fb.height as _,
        bpp: (fb.cpp * 8) as _,
        stride: fb.pitch as _,
        addr: unsafe { MmioView::new(fb.base, fb.pitch * fb.height, VmCacheType::WriteCombine) },
        obj_counter: IdAllocator::new(),
    });

    // Initialize DRM objects and store them in the device
    let crtc = Arc::new(Crtc::new(device.obj_counter.alloc()));
    let encoder = Arc::new(Encoder::new(
        device.obj_counter.alloc(),
        vec![crtc.clone()],
        crtc.clone(),
    ));

    // Create a primary plane for atomic modesetting
    let plane = Arc::new(Plane::new(
        device.obj_counter.alloc(),
        vec![crtc.clone()],
        DRM_PLANE_TYPE_PRIMARY,
        vec![DRM_FORMAT_XRGB8888],
    ));
    {
        let mut state = plane.state.lock();
        state.crtc_w = fb.width as u32;
        state.crtc_h = fb.height as u32;
        state.src_w = (fb.width as u32) << 16;
        state.src_h = (fb.height as u32) << 16;
    }
    let mut modes = vec![synthesize_preferred_mode(fb.width as u32, fb.height as u32)];
    modes.extend(
        DMT_MODES
            .iter()
            .filter(|&x| x.hdisplay == fb.width as _ && x.vdisplay == fb.height as _)
            .cloned(),
    );

    let connector = Arc::new(Connector::new(
        device.obj_counter.alloc(),
        drm_mode_connector_state::Connected,
        modes,
        vec![encoder.clone()],
        drm_mode_connector_type::Virtual,
        device
            .state
            .next_connector_type_id(drm_mode_connector_type::Virtual),
    ));

    let cursor_plane = Arc::new(Plane::new(
        device.obj_counter.alloc(),
        vec![crtc.clone()],
        DRM_PLANE_TYPE_CURSOR,
        vec![DRM_FORMAT_ARGB8888],
    ));

    device.state.crtcs.lock().push(crtc);
    device.state.encoders.lock().push(encoder);
    device.state.connectors.lock().push(connector);
    device.state.planes.lock().push(plane.clone());
    device.state.planes.lock().push(cursor_plane);

    super::register(device).expect("Unable to create plainfb DRM card");
}
