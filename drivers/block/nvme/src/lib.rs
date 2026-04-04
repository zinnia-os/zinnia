#![no_std]

use crate::controller::Controller;
use core::sync::atomic::AtomicUsize;
use zinnia::{
    alloc::format,
    core::sync::atomic::Ordering,
    device::block::register_block_device,
    error, log,
    memory::{MmioView, PhysAddr},
    posix::errno::{EResult, Errno},
    system::pci::{DeviceView, Driver, PciBar, PciVariant},
};

mod command;
mod controller;
mod error;
mod namespace;
mod queue;
mod spec;

static NVME_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn probe(_: &PciVariant, view: DeviceView<'static>) -> EResult<()> {
    log!("Probing NVMe device on {}", view.address());
    let bar = view.bar(0).ok_or(Errno::ENXIO)?;
    let (addr, size) = match bar {
        PciBar::Mmio32 { address, size, .. } => (address as usize, size),
        PciBar::Mmio64 { address, size, .. } => (address as _, size),
        _ => unreachable!("PCI NVMe devices are MMIO-only"),
    };
    let regs = unsafe { MmioView::new(PhysAddr::new(addr as _), size) };

    // TODO: Support legacy PCI interrupts.
    // TODO: Setup MSI-X.
    // let mut cap = view
    //     .capabilities()
    //     .filter_map(|mut x| x.msix())
    //     .next()
    //     .ok_or(Errno::ENXIO)?;

    let controller = match Controller::new_pci(regs) {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to probe controller: {e}");
            return Err(Errno::ENODEV);
        }
    };

    // Reset the controller to initialize all queues and other structures.
    if let Err(e) = controller.reset() {
        error!("Failed to reset controller: {e}");
        return Err(Errno::ENODEV);
    };

    if let Err(e) = controller.identify() {
        error!("Failed to identify controller: {e}");
        return Err(Errno::ENODEV);
    };

    let namespaces = match controller.scan_namespaces() {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to identify controller: {e}");
            return Err(Errno::ENODEV);
        }
    };

    let nvme_id = NVME_COUNTER.fetch_add(1, Ordering::SeqCst);
    for ns in namespaces {
        let path = format!("nvme{}n{}", nvme_id, ns.get_id());
        register_block_device(&path, ns)?;
    }

    Ok(())
}

static DRIVER: Driver = Driver {
    name: "nvme",
    probe,
    variants: &[PciVariant::new().class(1).sub_class(8).function(2)],
};

fn main(_cmdline: &str) {
    _ = DRIVER.register();
}

zinnia::module!("NVMe block devices", "Marvin Friedrich", main);
