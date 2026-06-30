mod parser;
mod usage;

use super::{Device, Driver, Endpoint, Interface, spec};
use crate::{
    device::input::{EventDevice, EventDeviceOps},
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::task::Task,
    uapi::input::{BUS_USB, EV_SYN, InputAbsinfo, InputId},
};
use alloc::{string::String, sync::Arc, vec, vec::Vec};
use parser::Parser;

#[initgraph::task(name = "device.usb.hid", depends = [crate::memory::MEMORY_STAGE])]
pub fn HID_STAGE() {
    DRIVER.register();
}

static DRIVER: Driver = Driver {
    name: "usb-hid",
    probe,
    attach,
    detach,
};

const HID_CLASS: u8 = 0x03;
const HID_SUBCLASS_BOOT: u8 = 0x01;
const DESC_TYPE_HID_REPORT: u8 = 0x22;
const HID_REQUEST_SET_IDLE: u8 = 0x0a;
const HID_REQUEST_SET_PROTOCOL: u8 = 0x0b;
/// Largest report descriptor we will fetch.
const MAX_REPORT_DESC: usize = 4096;

fn probe(_device: Arc<Device>, interface: &Interface) -> EResult<()> {
    if interface.desc.interface_class == HID_CLASS {
        Ok(())
    } else {
        Err(Errno::ENODEV)
    }
}

fn attach(device: Arc<Device>, interface: &Interface) -> EResult<()> {
    // Find the interrupt IN endpoint (the report endpoint).
    let endpoint = interface
        .endpoints
        .iter()
        .find(|ep| ep.desc.endpoint_address & 0x80 != 0 && ep.desc.attributes & 0x03 == 3);
    let Some(endpoint) = endpoint else {
        warn!("No interrupt IN endpoint on interface");
        return Err(Errno::ENODEV);
    };

    // Copy out what the worker needs.
    let ep_desc = endpoint.desc;
    let interface_desc = interface.desc;
    let device = device.clone();

    Task::run(move |_| {
        CpuData::get()
            .scheduler
            .block_on(hid_worker(device, ep_desc, interface_desc));
    })?;

    Ok(())
}

fn detach(_device: Arc<Device>, _interface: &Interface) -> EResult<()> {
    // TODO: signal the worker to stop. It currently exits when transfers fail.
    Ok(())
}

async fn hid_worker(
    device: Arc<Device>,
    ep_desc: spec::EndpointDescriptor,
    interface_desc: spec::InterfaceDescriptor,
) {
    let interface_number = interface_desc.interface_number;
    let endpoint = Endpoint {
        desc: ep_desc,
        ss_companion: None,
    };

    // Identify the device for the input layer.
    let (vendor, product, product_string_index) =
        (*device.descriptor.lock()).map_or((0, 0, 0), |d| (d.vendor_id, d.product_id, d.product));
    let device_name = match device.get_string(interface_desc.interface).await {
        Some(name) => Some(name),
        None => device.get_string(product_string_index).await,
    };

    // Fetch the report descriptor.
    let mut report_desc = vec![0u8; MAX_REPORT_DESC];
    let get_report_desc = spec::Setup {
        request_type: 0x81, // IN | standard | interface
        request: spec::USB_REQUEST_GET_DESCRIPTOR as u8,
        value: (DESC_TYPE_HID_REPORT as u16) << 8,
        index: interface_number as u16,
        length: report_desc.len() as u16,
    };
    let length = match device
        .control_transfer(get_report_desc, &mut report_desc)
        .await
    {
        Ok(n) if n > 0 => n,
        other => {
            warn!("Failed to get report descriptor: {:?}", other);
            return;
        }
    };
    report_desc.truncate(length);

    if interface_desc.interface_sub_class == HID_SUBCLASS_BOOT {
        let set_idle = spec::Setup {
            request_type: 0x21, // OUT | class | interface
            request: HID_REQUEST_SET_IDLE,
            value: 0,
            index: interface_number as u16,
            length: 0,
        };
        let _ = device.control_transfer(set_idle, &mut []).await;
        let set_protocol = spec::Setup {
            request_type: 0x21,
            request: HID_REQUEST_SET_PROTOCOL,
            value: 1, // report protocol
            index: interface_number as u16,
            length: 0,
        };
        let _ = device.control_transfer(set_protocol, &mut []).await;
    }

    let mut parser = match Parser::parse(&report_desc) {
        Ok(parser) => parser,
        Err(()) => {
            warn!("Failed to parse report descriptor");
            return;
        }
    };
    if parser.applications.is_empty() {
        warn!("Report descriptor has no applications");
        return;
    }

    log!(
        "{:04x}:{:04x} {} application(s), {} report(s)",
        vendor,
        product,
        parser.applications.len(),
        if parser.reports.is_empty() {
            parser.inputs.len()
        } else {
            parser.reports.len()
        }
    );

    // One evdev input device per HID application.
    let mut devices: Vec<Arc<EventDevice>> = Vec::new();
    for caps in parser.build_caps() {
        let ops = Arc::new(HidInputDevice {
            name: device_name.as_deref().unwrap_or(caps.name).into(),
            id: InputId {
                bustype: BUS_USB,
                vendor,
                product,
                version: 1,
            },
            ev: caps.ev | (1 << EV_SYN),
            keys: caps.keys,
            rels: caps.rels,
            abs: caps.abs,
            abs_info: caps.abs_info,
        });
        let event_device = EventDevice::new(ops);
        if event_device.register_device().is_err() {
            warn!("Failed to register input device");
            return;
        }
        devices.push(event_device);
    }

    // Poll the interrupt endpoint and decode reports.
    let packet_size = ep_desc.max_packet_size().max(1) as usize;
    let mut buffer = vec![0u8; packet_size];
    let mut failures = 0u32;
    loop {
        match device.interrupt_transfer(&endpoint, &mut buffer).await {
            Ok(transferred) => {
                failures = 0;
                if transferred > 0 {
                    parser.parse_report(&buffer[..transferred], &devices);
                }
            }
            Err(e) => {
                failures += 1;
                if failures > 16 {
                    warn!("Interrupt transfers failing, stopping worker: {:?}", e);
                    return;
                }
            }
        }
    }
}

struct HidInputDevice {
    name: String,
    id: InputId,
    ev: u32,
    keys: Vec<u8>,
    rels: Vec<u8>,
    abs: Vec<u8>,
    abs_info: Vec<(i32, i32)>,
}

impl EventDeviceOps for HidInputDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> InputId {
        self.id
    }

    fn supported_events(&self) -> u32 {
        self.ev
    }

    fn supported_keys(&self) -> &[u8] {
        &self.keys
    }

    fn supported_rel(&self) -> &[u8] {
        &self.rels
    }

    fn supported_abs(&self) -> &[u8] {
        &self.abs
    }

    fn abs_info(&self, code: u16) -> InputAbsinfo {
        let (minimum, maximum) = self.abs_info.get(code as usize).copied().unwrap_or((0, 0));
        // Sensible initial value within range.
        let value = if minimum > 0 {
            minimum
        } else if maximum < 0 {
            maximum
        } else {
            0
        };
        InputAbsinfo {
            value,
            minimum,
            maximum,
            fuzz: 0,
            flat: 0,
            resolution: 0,
        }
    }
}
