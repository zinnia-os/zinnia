use super::{
    Device, Driver, Endpoint, HubInfo, Interface, Speed, Status, UsbResult,
    hub::{Hub, HubOps},
    spec,
    spec::{PortChange, PortFeature, PortStatus},
};
use crate::{
    clock,
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::task::Task,
};
use alloc::{boxed::Box, format, sync::Arc, vec};
use async_trait::async_trait;
use core::sync::atomic::{AtomicBool, Ordering};

/// Registers the hub class driver with the USB core during boot.
#[initgraph::task(name = "device.usb.hub", depends = [crate::memory::MEMORY_STAGE])]
pub fn HUB_STAGE() {
    DRIVER.register();
}

static DRIVER: Driver = Driver {
    name: "hub",
    probe,
    attach,
    detach,
};

const HUB_REQUEST_GET_STATUS: u8 = 0;
const HUB_REQUEST_CLEAR_FEATURE: u8 = 1;
const HUB_REQUEST_SET_FEATURE: u8 = 3;

/// Per-hub driver state, stored in [`Interface::driver_data`].
struct HubClassState {
    hub: Arc<Hub>,
    /// Stops the status-change worker on detach.
    stop: AtomicBool,
}

fn probe(_device: Arc<Device>, interface: &Interface) -> EResult<()> {
    if interface.desc.interface_class == spec::USB_CLASS_HUB {
        Ok(())
    } else {
        Err(Errno::ENODEV)
    }
}

fn attach(device: Arc<Device>, interface: &Interface) -> EResult<()> {
    // Filled in during enumeration, before the interface drivers attach.
    let Some(info) = *device.hub_info.lock() else {
        warn!("device has no hub descriptor");
        return Err(Errno::ENODEV);
    };

    // Port changes are reported on the (sole) interrupt IN endpoint.
    let endpoint = interface
        .endpoints
        .iter()
        .find(|ep| ep.desc.endpoint_address & 0x80 != 0 && ep.desc.attributes & 0x03 == 3);
    let Some(endpoint) = endpoint else {
        warn!("no interrupt IN endpoint on interface");
        return Err(Errno::ENODEV);
    };
    let ep_desc = endpoint.desc;

    let usb_version = (*device.descriptor.lock()).map_or(0, |d| d.usb);
    let name = format!(
        "usb-hub-{:x}.{:x}{:x}",
        usb_version >> 8,
        (usb_version >> 4) & 0xf,
        usb_version & 0xf
    );

    let hub = Hub::new(
        name,
        device.parent_hub.controller.clone(),
        Box::new(UsbHubOps {
            device: device.clone(),
        }),
        info.port_count,
        Some(device.clone()),
    )?;

    let state = Arc::new(HubClassState {
        hub,
        stop: AtomicBool::new(false),
    });
    *interface.driver_data.lock() = Some(state.clone());

    // All blocking work happens on a dedicated worker; `attach` only spawns.
    let device = device.clone();
    Task::run(move |_| {
        CpuData::get()
            .scheduler
            .block_on(hub_worker(device, ep_desc, state, info));
    })?;

    Ok(())
}

fn detach(_device: Arc<Device>, interface: &Interface) -> EResult<()> {
    let Some(state) = interface.driver_data.lock().take() else {
        return Ok(());
    };
    let Ok(state) = state.downcast::<HubClassState>() else {
        return Ok(());
    };

    state.stop.store(true, Ordering::Release);

    // Tear down the devices behind the hub, then let the hub worker exit once
    // it has drained the disconnects.
    for port in 0..state.hub.ports.len() as u8 {
        state.hub.handle_disconnect(port);
    }
    state.hub.stop();
    Ok(())
}

async fn hub_worker(
    device: Arc<Device>,
    ep_desc: spec::EndpointDescriptor,
    state: Arc<HubClassState>,
    info: HubInfo,
) {
    let hub = &state.hub;

    // Power on all ports, then wait for power to stabilize.
    if info.power_switched {
        for port in 0..info.port_count {
            if let Err(e) = hub
                .ops
                .set_port_feature(hub, port, PortFeature::PortPower)
                .await
            {
                warn!("failed to power port {}: {:?}", port + 1, e);
            }
        }
        if info.power_good_ms > 0 {
            clock::sleep(core::time::Duration::from_millis(info.power_good_ms as u64));
        }
    }

    // Pick up devices that were connected before we attached.
    for port in 0..info.port_count {
        match hub.ops.get_port_status(hub, port).await {
            Ok((status, change)) => {
                if status.contains(PortStatus::PortConnection) {
                    hub.handle_connect(port);
                }
                clear_port_changes(hub, port, change).await;
            }
            Err(e) => warn!("failed to get port {} status: {:?}", port + 1, e),
        }
    }

    // Watch the status-change endpoint: bit 0 reports hub-wide changes (which
    // we do not handle), bit N a change on port N.
    let endpoint = Endpoint {
        desc: ep_desc,
        ss_companion: None,
    };
    let mut bitmap = vec![0u8; (info.port_count as usize + 1).div_ceil(8)];
    let mut failures = 0u32;
    while !state.stop.load(Ordering::Acquire) {
        let transferred = match device.interrupt_transfer(&endpoint, &mut bitmap).await {
            Ok(transferred) => {
                failures = 0;
                transferred
            }
            Err(e) => {
                failures += 1;
                if failures > 16 {
                    warn!("status endpoint failing, stopping worker: {:?}", e);
                    break;
                }
                continue;
            }
        };

        for port in 0..info.port_count {
            let bit = port as usize + 1;
            if bit / 8 >= transferred || bitmap[bit / 8] & (1 << (bit % 8)) == 0 {
                continue;
            }

            let (status, change) = match hub.ops.get_port_status(hub, port).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("failed to get port {} status: {:?}", port + 1, e);
                    continue;
                }
            };

            if change.contains(PortChange::PortConnection) {
                if status.contains(PortStatus::PortConnection) {
                    hub.handle_connect(port);
                } else {
                    hub.handle_disconnect(port);
                }
            } else if change.contains(PortChange::PortReset) {
                hub.handle_reset(port, port_speed(&device, status));
            }
            clear_port_changes(hub, port, change).await;
        }
    }
}

/// The speed of a device attached to a hub port, per the port status bits.
fn port_speed(hub_device: &Device, status: PortStatus) -> Speed {
    if matches!(hub_device.speed, Speed::Super | Speed::SuperPlus) {
        Speed::Super
    } else if status.contains(PortStatus::LowSpeed) {
        Speed::Low
    } else if status.contains(PortStatus::HighSpeed) {
        Speed::High
    } else {
        Speed::Full
    }
}

/// Acknowledges the change bits set in `change`.
async fn clear_port_changes(hub: &Hub, port: u8, change: PortChange) {
    let features = [
        (PortChange::PortConnection, PortFeature::CPortConnection),
        (PortChange::PortEnable, PortFeature::CPortEnable),
        (PortChange::PortOverCurrent, PortFeature::CPortOverCurrent),
        (PortChange::PortReset, PortFeature::CPortReset),
    ];
    for (bit, feature) in features {
        if change.contains(bit)
            && let Err(e) = hub.ops.clear_port_feature(hub, port, feature).await
        {
            warn!("failed to clear change on port {}: {:?}", port + 1, e);
        }
    }
}

struct UsbHubOps {
    device: Arc<Device>,
}

impl UsbHubOps {
    async fn port_request(
        &self,
        request: u8,
        value: u16,
        port: u8,
        buf: &mut [u8],
    ) -> UsbResult<usize> {
        let dir = if buf.is_empty() {
            spec::USB_REQUEST_DIR_TO_DEVICE
        } else {
            spec::USB_REQUEST_DIR_TO_HOST
        };
        let setup = spec::Setup {
            request_type: (dir | spec::USB_REQUEST_CLASS | spec::USB_REQUEST_RECIP_OTHER) as u8,
            request,
            value,
            index: port as u16 + 1,
            length: buf.len() as u16,
        };
        self.device.control_transfer(setup, buf).await
    }
}

#[async_trait(?Send)]
impl HubOps for UsbHubOps {
    async fn get_port_status(&self, _hub: &Hub, port: u8) -> UsbResult<(PortStatus, PortChange)> {
        let mut buf = [0u8; 4];
        let transferred = self
            .port_request(HUB_REQUEST_GET_STATUS, 0, port, &mut buf)
            .await?;
        if transferred < buf.len() {
            return Err(Status::Error);
        }

        let status = u16::from_le_bytes([buf[0], buf[1]]);
        let change = u16::from_le_bytes([buf[2], buf[3]]);
        Ok((
            PortStatus::from_bits_truncate(status),
            PortChange::from_bits_truncate(change),
        ))
    }

    async fn set_port_feature(&self, _hub: &Hub, port: u8, feature: PortFeature) -> UsbResult<()> {
        self.port_request(HUB_REQUEST_SET_FEATURE, feature as u16, port, &mut [])
            .await
            .map(|_| ())
    }

    async fn clear_port_feature(
        &self,
        _hub: &Hub,
        port: u8,
        feature: PortFeature,
    ) -> UsbResult<()> {
        self.port_request(HUB_REQUEST_CLEAR_FEATURE, feature as u16, port, &mut [])
            .await
            .map(|_| ())
    }
}
