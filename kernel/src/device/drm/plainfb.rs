use super::{
    DeviceState,
    object::{AtomicState, BufferObject, Connector, Crtc, Encoder, Plane},
};
use crate::{
    arch,
    boot::BootInfo,
    device::drm::{Device, DrmFile, IdAllocator, modes::DMT_MODES, object::Framebuffer},
    memory::{
        MemoryObject, MmioView, PhysAddr,
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
    },
    posix::errno::{EResult, Errno},
    uapi::drm::{
        DRM_FORMAT_XRGB8888, DRM_PLANE_TYPE_PRIMARY, drm_mode_connector_state,
        drm_mode_connector_type,
    },
};
use alloc::{sync::Arc, vec::Vec};
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

    fn driver_version(&self) -> (u32, u32, u32) {
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
        (width == self.width).ok_or(Errno::EINVAL)?;
        (height == self.height).ok_or(Errno::EINVAL)?;
        (bpp == self.bpp).ok_or(Errno::EINVAL)?;

        let size = self.stride as usize * self.height as usize;

        // Allocate physical memory for the buffer
        let buffer_addr =
            KernelAlloc::alloc_bytes(size, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;

        Ok((
            Arc::new(PlainDumbBuffer {
                id: 0,
                size,
                width,
                height,
                addr: buffer_addr,
            }),
            self.stride,
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
        // Copy from each buffer to the framebuffer
        for crtc_state in state.crtc_states.values() {
            if let Some(ref framebuffer) = crtc_state.framebuffer
                && let Some(buffer) =
                    (framebuffer.buffer.as_ref() as &dyn Any).downcast_ref::<PlainDumbBuffer>()
            {
                // Copy from buffer to framebuffer
                let src = buffer.addr.as_hhdm::<u8>();
                let dst = self.addr.base() as _;
                let size = buffer.size.min(self.addr.len());

                unsafe {
                    core::ptr::copy_nonoverlapping(src, dst, size);
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
        Some(self.addr + (page_index * arch::virt::get_page_size()))
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
        addr: unsafe { MmioView::new(fb.base, fb.pitch * fb.height) },
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

    let connector = Arc::new(Connector::new(
        device.obj_counter.alloc(),
        drm_mode_connector_state::Connected,
        DMT_MODES
            .iter()
            .filter(|&x| x.hdisplay == fb.width as _ && x.vdisplay == fb.height as _)
            .cloned()
            .collect::<Vec<_>>(),
        vec![encoder.clone()],
        drm_mode_connector_type::Virtual,
    ));

    device.state.crtcs.lock().push(crtc);
    device.state.encoders.lock().push(encoder);
    device.state.connectors.lock().push(connector);
    device.state.planes.lock().push(plane.clone());

    super::register(DrmFile::new(device)).expect("Unable to create plainfb DRM card");
}
