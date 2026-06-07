#![no_std]

use crate::controller::Controller;
use core::sync::atomic::AtomicUsize;
use zinnia::{
    alloc::{boxed::Box, format},
    core::sync::atomic::Ordering,
    device::{
        block::register_block_device,
        pci::{DeviceView, Driver, PciBar, PciVariant, common},
    },
    error,
    irq::{IrqHandler, Status},
    log,
    memory::{MmioView, PhysAddr, VmCacheType},
    posix::errno::{EResult, Errno},
};

mod command;
mod controller;
mod error;
mod namespace;
mod queue;
mod spec;

static NVME_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct NvmeIrqHandler;

impl IrqHandler for NvmeIrqHandler {
    fn raise(&mut self) -> Status {
        Status::Handled
    }
}

fn probe(_: &PciVariant, mut view: DeviceView<'static>) -> EResult<()> {
    log!("Probing NVMe device on {}", view.address());

    // Enable MMIO decoding and DMA. We only support MSI-X/polling, so keep legacy INTx disabled.
    let cmd = view
        .access()
        .read16(view.address(), common::REG1.offset() as u32);
    view.access().write16(
        view.address(),
        common::REG1.offset() as u32,
        cmd | (1 << 1) | (1 << 2) | (1 << 10),
    );

    let irq_line = {
        match view.setup_msix() {
            Ok(line) => Some(line),
            Err(_) => {
                log!("NVMe MSI-X setup failed, falling back to polling completions");
                None
            }
        }
    };

    let bar = view.bar(0).ok_or(Errno::ENXIO)?;
    let (addr, size) = match bar {
        PciBar::Mmio32 { address, size, .. } => (address as usize, size),
        PciBar::Mmio64 { address, size, .. } => (address as _, size),
        _ => unreachable!("PCI NVMe devices are MMIO-only"),
    };
    let regs = unsafe { MmioView::new(PhysAddr::new(addr as _), size, VmCacheType::Uncacheable) };

    let controller = match Controller::new_pci(regs) {
        Ok(x) => x,
        Err(e) => {
            error!("Failed to probe controller: {e}");
            return Err(Errno::ENODEV);
        }
    };

    if let Some(irq_line) = &irq_line {
        irq_line.attach(Box::new(NvmeIrqHandler));
        irq_line.unmask();
    }

    // Reset the controller to initialize all queues and other structures.
    if let Err(e) = controller.reset(irq_line.as_ref().map(|_| 0)) {
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
