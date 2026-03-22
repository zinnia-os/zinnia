#![no_std]

use crate::device::VirtioGpuDevice;
use zinnia::{
    alloc::sync::Arc,
    arch,
    device::drm::DrmFile,
    error, log,
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr},
    posix::errno::{EResult, Errno},
    system::pci::{DeviceView, Driver, PciVariant},
    util::mutex::spin::SpinMutex,
};
use virtio::{VirtQueue, VirtioDevice};

mod device;
mod spec;

use spec::*;

fn probe(_: &PciVariant, view: DeviceView<'static>) -> EResult<()> {
    log!("Probing VirtIO GPU device on {}", view.address());
    let mut virtio_dev = VirtioDevice::new_pci(view.clone())?;

    let device_features = virtio_dev.get_device_features(0);
    log!("Features: {:08x}", device_features);

    // We can accept the features as-is for now
    virtio_dev.set_driver_features(0, device_features & VIRTIO_GPU_SUPPORTED_FEATURES);

    // Setup virtqueues (control and cursor)
    let num_queues = virtio_dev.num_queues();
    if num_queues < 2 {
        error!("VirtIO GPU requires at least 2 queues");
        return Err(Errno::ENODEV);
    }

    // Allocate main queue
    let ctrl_queue_size = virtio_dev.get_queue_max_size(0);
    let (ctrl_desc, ctrl_avail, ctrl_used) = allocate_queue_memory(ctrl_queue_size)?;
    let ctrl_notify_off =
        virtio_dev.setup_queue(0, ctrl_queue_size, ctrl_desc, ctrl_avail, ctrl_used)?;
    let ctrl_queue = unsafe { VirtQueue::new(ctrl_queue_size, ctrl_desc, ctrl_avail, ctrl_used) };

    // Allocate cursor queue
    let cursor_queue_size = virtio_dev.get_queue_max_size(1);
    let (cursor_desc, cursor_avail, cursor_used) = allocate_queue_memory(cursor_queue_size)?;
    let cursor_notify_off =
        virtio_dev.setup_queue(1, cursor_queue_size, cursor_desc, cursor_avail, cursor_used)?;
    let cursor_queue =
        unsafe { VirtQueue::new(cursor_queue_size, cursor_desc, cursor_avail, cursor_used) };

    // Finalize device initialization
    virtio_dev.finalize()?;

    // Create the GPU device
    let gpu_device = Arc::new(VirtioGpuDevice::new(
        virtio_dev,
        SpinMutex::new(ctrl_queue),
        ctrl_notify_off,
        SpinMutex::new(cursor_queue),
        cursor_notify_off,
    )?);

    // Create DRM file handle
    let drm_file = DrmFile::new(gpu_device.clone());

    // Initialize DRM objects (CRTCs, encoders, connectors)
    gpu_device.initialize_drm_objects(&drm_file)?;

    zinnia::device::drm::register(drm_file)?;

    Ok(())
}

fn allocate_queue_memory(queue_size: u16) -> EResult<(PhysAddr, PhysAddr, PhysAddr)> {
    let queue_size_usize = queue_size as usize;
    let page_size = arch::virt::get_page_size();

    // Descriptor table: 16 bytes per entry
    let desc_size = queue_size_usize * 16;
    let desc_pages = desc_size.div_ceil(page_size);
    let desc_addr =
        KernelAlloc::alloc(desc_pages, AllocFlags::Zeroed).map_err(|_| Errno::ENOMEM)?;

    // Available ring: 2 + 2 + 2*queue_size + 2 bytes (with padding)
    let avail_size = 6 + 2 * queue_size_usize;
    let avail_pages = avail_size.div_ceil(page_size);
    let avail_addr =
        KernelAlloc::alloc(avail_pages, AllocFlags::Zeroed).map_err(|_| Errno::ENOMEM)?;

    // Used ring: 2 + 2 + 8*queue_size + 2 bytes (with padding)
    let used_size = 6 + 8 * queue_size_usize;
    let used_pages = used_size.div_ceil(page_size);
    let used_addr =
        KernelAlloc::alloc(used_pages, AllocFlags::Zeroed).map_err(|_| Errno::ENOMEM)?;

    Ok((desc_addr, avail_addr, used_addr))
}

static DRIVER: Driver = Driver {
    name: "virtio_gpu",
    probe,
    variants: &[PciVariant::new().vendor(0x1AF4).device(0x1050)],
};

zinnia::module!("VirtIO GPU driver", "Marvin Friedrich", main);

pub fn main() {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load VirtIO GPU driver: {:?}", e),
    }
}
