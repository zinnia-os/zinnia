#![no_std]

use crate::{
    device::{Geometry, VirtioBlkDevice},
    queue::RequestQueue,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use virtio::VirtioDevice;
use zinnia::{
    alloc::{boxed::Box, format, sync::Arc},
    device::pci::{DeviceView, Driver, PciVariant},
    error,
    irq::{IrqHandler, Status},
    log,
    posix::errno::{EResult, Errno},
};

mod device;
mod error;
mod queue;
mod spec;

const MSIX_VECTOR: u16 = 0;
const MIN_QUEUE_SIZE: u16 = 4;

static BLK_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct VirtioBlkIrqHandler {
    queue: Arc<RequestQueue>,
}

impl IrqHandler for VirtioBlkIrqHandler {
    fn raise(&mut self) -> Status {
        self.queue.drain();
        Status::Handled
    }
}

fn enable_bus_mastering(view: &mut DeviceView<'static>) {
    let command = view.access().read32(view.address(), 0x04) as u16;
    let updated = command | (1 << 1) | (1 << 2);
    view.access().write32(view.address(), 0x04, updated as u32);
}

fn bind_interrupts(virtio: &mut VirtioDevice) -> EResult<bool> {
    if virtio.set_queue_msix_vector(spec::REQUEST_QUEUE, MSIX_VECTOR)? != MSIX_VECTOR {
        return Ok(false);
    }
    virtio.set_config_msix_vector(0xFFFF)?;
    Ok(true)
}

fn probe(_: &PciVariant, mut view: DeviceView<'static>) -> EResult<()> {
    log!("Probing VirtIO block device on {}", view.address());

    enable_bus_mastering(&mut view);
    let irq_line = view.setup_irq().ok();

    let mut virtio = VirtioDevice::new_pci(view)?;

    let device_lo = virtio.get_device_features(0)?;
    let device_hi = virtio.get_device_features(1)?;
    if device_hi & spec::VIRTIO_F_VERSION_1_LO == 0 {
        error!("VirtIO block device does not implement the 1.0 interface");
        return Err(Errno::ENOTSUP);
    }

    let driver_lo = device_lo & spec::SUPPORTED_FEATURES;
    virtio.set_driver_features(0, driver_lo)?;
    virtio.set_driver_features(1, spec::VIRTIO_F_VERSION_1_LO)?;
    virtio.finalize_features()?;
    log!("Negotiated features: {driver_lo:#010x}");

    if virtio.num_queues()? == 0 {
        error!("VirtIO block device exposes no request queue");
        return Err(Errno::ENODEV);
    }

    let queue = virtio.setup_queue(spec::REQUEST_QUEUE)?;
    let queue_size = queue.queue_size();
    if queue_size < MIN_QUEUE_SIZE {
        error!("Request queue is too small ({queue_size} descriptors)");
        return Err(Errno::ENODEV);
    }

    let geometry = Geometry::read(&virtio, driver_lo, queue_size)?;

    let irq_line = match irq_line {
        Some(line) => bind_interrupts(&mut virtio)?.then_some(line),
        None => None,
    };
    if irq_line.is_none() {
        log!("Falling back to polled completions");
    }

    virtio.set_driver_ok()?;

    let requests = Arc::new(RequestQueue::new(virtio, queue, irq_line.is_none())?);

    if let Some(line) = irq_line.as_ref() {
        line.attach(Box::new(VirtioBlkIrqHandler {
            queue: requests.clone(),
        }));
        line.unmask();
    }

    let index = BLK_COUNTER.fetch_add(1, Ordering::SeqCst);
    let blk = Arc::new(VirtioBlkDevice::new(requests, geometry, index as u32));

    zinnia::device::block::register_block_device(&format!("virtblk{index}"), blk)
}

const BASE_VARIANT: PciVariant = PciVariant::new().vendor(0x1AF4);

static DRIVER: Driver = Driver {
    name: "virtio_blk",
    probe,
    variants: &[BASE_VARIANT.device(0x1001), BASE_VARIANT.device(0x1042)],
};

zinnia::module!("VirtIO block driver", "Marvin Friedrich", main);

pub fn main(_cmdline: &str) {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load VirtIO block driver: {:?}", e),
    }
}
