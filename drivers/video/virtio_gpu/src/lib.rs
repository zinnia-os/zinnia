#![no_std]

use crate::{
    device::VirtioGpuDevice,
    queue::{ControlQueue, CursorQueue},
};
use virtio::VirtioDevice;
use zinnia::{
    alloc::{boxed::Box, sync::Arc},
    device::pci::{DeviceView, Driver, PciVariant},
    error,
    irq::{IrqHandler, Status},
    log,
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};

mod command;
mod context;
mod device;
mod dma;
mod error;
mod queue;
mod resource;
mod spec;

const CONTROL_QUEUE: u16 = 0;
const CURSOR_QUEUE: u16 = 1;
const MSIX_VECTOR: u16 = 0;

struct VirtioGpuIrqHandler {
    ctrl: Arc<ControlQueue>,
    cursor: Arc<CursorQueue>,
}

impl IrqHandler for VirtioGpuIrqHandler {
    fn raise(&mut self) -> Status {
        self.ctrl.drain();
        self.cursor.reap();
        Status::Handled
    }
}

fn enable_bus_mastering(view: &mut DeviceView<'static>) {
    let command = view.access().read32(view.address(), 0x04) as u16;
    let updated = command | (1 << 1) | (1 << 2);
    view.access().write32(view.address(), 0x04, updated as u32);
}

fn probe(_: &PciVariant, mut view: DeviceView<'static>) -> EResult<()> {
    log!("Probing VirtIO GPU device on {}", view.address());

    enable_bus_mastering(&mut view);
    let irq_line = view.setup_irq().ok();

    let mut virtio = VirtioDevice::new_pci(view)?;

    let device_lo = virtio.get_device_features(0)?;
    let device_hi = virtio.get_device_features(1)?;

    let driver_lo = device_lo & (spec::VIRTIO_GPU_F_VIRGL | spec::VIRTIO_GPU_F_EDID);
    let driver_hi = device_hi & spec::VIRTIO_F_VERSION_1_LO;

    virtio.set_driver_features(0, driver_lo)?;
    virtio.set_driver_features(1, driver_hi)?;
    virtio.finalize_features()?;

    let accelerated = driver_lo & spec::VIRTIO_GPU_F_VIRGL != 0;
    log!(
        "Negotiated features lo: 0x{:08x}, hi: 0x{:08x}, virgl {}",
        driver_lo,
        driver_hi,
        if accelerated { "enabled" } else { "disabled" }
    );

    if virtio.num_queues()? < 2 {
        error!("VirtIO GPU requires at least 2 queues");
        return Err(Errno::ENODEV);
    }

    let control_queue = virtio.setup_queue(CONTROL_QUEUE)?;
    let cursor_queue = virtio.setup_queue(CURSOR_QUEUE)?;

    let irq_line = match irq_line {
        Some(line) => bind_interrupts(&mut virtio)?.then_some(line),
        None => None,
    };
    if irq_line.is_none() {
        log!("Falling back to polled completions");
    }

    let num_capsets = virtio.read_config32(spec::config::NUM_CAPSETS)?;

    virtio.set_driver_ok()?;

    let virtio = Arc::new(SpinMutex::new(virtio));
    let ctrl = Arc::new(ControlQueue::new(
        virtio.clone(),
        control_queue,
        irq_line.is_none(),
    )?);
    let cursor = Arc::new(CursorQueue::new(virtio, cursor_queue));

    if let Some(line) = irq_line.as_ref() {
        line.attach(Box::new(VirtioGpuIrqHandler {
            ctrl: ctrl.clone(),
            cursor: cursor.clone(),
        }));
        line.unmask();
    }

    let gpu = Arc::new(VirtioGpuDevice::new(
        ctrl,
        cursor,
        accelerated,
        num_capsets,
    )?);

    zinnia::device::drm::register(gpu)
}

fn bind_interrupts(virtio: &mut VirtioDevice) -> EResult<bool> {
    if virtio.set_queue_msix_vector(CONTROL_QUEUE, MSIX_VECTOR)? != MSIX_VECTOR {
        return Ok(false);
    }
    if virtio.set_queue_msix_vector(CURSOR_QUEUE, MSIX_VECTOR)? != MSIX_VECTOR {
        return Ok(false);
    }
    virtio.set_config_msix_vector(0xFFFF)?;
    Ok(true)
}

static DRIVER: Driver = Driver {
    name: "virtio_gpu",
    probe,
    variants: &[
        PciVariant::new().vendor(0x1AF4).device(0x1050),
        PciVariant::new().vendor(0x1AF4).device(0x1010),
    ],
};

zinnia::module!("VirtIO GPU driver", "Marvin Friedrich", main);

pub fn main(_cmdline: &str) {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load VirtIO GPU driver: {:?}", e),
    }
}
