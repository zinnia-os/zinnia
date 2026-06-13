#![no_std]

use zinnia::{
    device::pci::{self, DeviceView, PciBar, PciVariant},
    error, log,
    posix::errno::{EResult, Errno},
};

mod device;
mod hub;
mod ring;
mod spec;
mod transfer;

zinnia::module!("xHCI USB Controller", "Marvin Friedrich", main);

pub fn main(_cmdline: &str) {
    match PCI_DRIVER.register() {
        Ok(_) => (),
        Err(e) => error!("Unable to load driver: {:?}", e),
    }
}

static PCI_DRIVER: pci::Driver = pci::Driver {
    name: "xhci_pci",
    // Serial, USB, xHCI
    variants: &[PciVariant::new().class(0xC).sub_class(0x3).function(0x30)],
    probe,
};

fn probe(variant: &PciVariant, mut view: DeviceView<'static>) -> EResult<()> {
    log!("Setting up xHCI controller on {}", view.address());

    let bar = view.bar(0).ok_or(Errno::ENXIO)?;
    let (bar_addr, bar_size) = match bar {
        PciBar::Mmio32 { address, size, .. } => (address as usize, size),
        PciBar::Mmio64 { address, size, .. } => (address as usize, size),
        PciBar::Io { .. } => return Err(Errno::EINVAL),
    };

    let irq_line = view.setup_msix()?;

    Ok(())
}

fn handoff(bar: &PciBar) -> EResult<()> {
    Ok(())
}
