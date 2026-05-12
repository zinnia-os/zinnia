#![no_std]

use crate::device::VirtioGpuDevice;
use virtio::VirtioDevice;
use zinnia::{
    alloc::sync::Arc,
    device::pci::{DeviceView, Driver, PciVariant},
    error, log,
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};

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
    virtio_dev.finalize_features()?;

    // Setup virtqueues (control and cursor)
    let num_queues = virtio_dev.num_queues();
    if num_queues < 2 {
        error!("VirtIO GPU requires at least 2 queues");
        return Err(Errno::ENODEV);
    }

    // Main queue
    let ctrl_queue = virtio_dev.setup_queue(0)?;

    // Cursor queue
    let cursor_queue = virtio_dev.setup_queue(1)?;

    // Finalize device initialization
    virtio_dev.set_driver_ok();

    // Create the GPU device
    let gpu_device = Arc::new(VirtioGpuDevice::new(
        virtio_dev,
        SpinMutex::new(ctrl_queue),
        SpinMutex::new(cursor_queue),
    )?);

    // Initialize DRM objects (CRTCs, encoders, connectors)
    gpu_device.initialize_drm_objects()?;

    zinnia::device::drm::register(gpu_device)?;

    Ok(())
}

static DRIVER: Driver = Driver {
    name: "virtio_gpu",
    probe,
    variants: &[PciVariant::new().vendor(0x1AF4).device(0x1050)],
};

zinnia::module!("VirtIO GPU driver", "Marvin Friedrich", main);

pub fn main(_cmdline: &str) {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load VirtIO GPU driver: {:?}", e),
    }
}
