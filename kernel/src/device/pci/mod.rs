use crate::memory::view::MemoryView;
use alloc::vec::Vec;
use config::common::{DEVICE_ID, REG0, VENDOR_ID};

mod config;
pub use config::*;
mod device;
pub use device::*;
mod driver;
pub use driver::*;

/// Initializes the PCI subsystem.
#[initgraph::task(name = "system.pci")]
#[cfg_attr(
    feature = "acpi",
    initgraph::task(depends = [super::acpi::INIT_STAGE])
)]
pub fn PCI_STAGE() {
    log!("Scanning PCI devices");

    let mut devices = PCI_DEVICES.lock();
    for access in ACCESS.get().iter() {
        for bus in access.start_bus()..=access.end_bus() {
            for slot in 0..32 {
                let addr = Address {
                    segment: access.segment(),
                    bus,
                    slot,
                    function: 0,
                };
                scan_device(addr, access.view_for_device(addr).unwrap(), &mut devices);
            }
        }
    }
}

fn scan_device(addr: Address, view: DeviceView<'_>, devices: &mut Vec<Address>) {
    let reg0 = view.read_reg(REG0).unwrap();

    let vendor_id = reg0.read_field(VENDOR_ID).value();
    if vendor_id == 0xFFFF || vendor_id == 0 {
        return;
    }

    let device_id = reg0.read_field(DEVICE_ID).value();
    if device_id == 0xFFFF || device_id == 0 {
        return;
    }

    log!(
        "{}: Vendor={:04x} Device={:04x}",
        addr,
        vendor_id,
        device_id
    );

    devices.push(addr);
}
