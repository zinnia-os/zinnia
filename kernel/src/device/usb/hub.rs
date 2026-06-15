use crate::{
    device::usb::{
        Controller, Device, Endpoint, HubInfo, Interface, Speed, Status, UsbResult,
        spec::{
            self, DescriptorType, EndpointDescriptor, HubDescriptor, InterfaceDescriptor,
            PortChange, PortFeature, PortStatus, SsEpCompanionDescriptor,
        },
    },
    percpu::CpuData,
    posix::errno::EResult,
    process::task::Task,
    util::{event::Event, mutex::spin::SpinMutex},
};
use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use async_trait::async_trait;
use core::{
    ptr::read_unaligned,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
};

const MAX_CONFIG_DESC_SIZE: usize = 4096;
const PENDING_NONE: u8 = 0;
const PENDING_CONNECT: u8 = 1;
const PENDING_DISCONNECT: u8 = 2;
const PENDING_RESET: u8 = 3;

/// Operations a hub must implement.
#[async_trait(?Send)]
pub trait HubOps {
    async fn get_port_status(&self, hub: &Hub, port: u8) -> UsbResult<(PortStatus, PortChange)>;

    async fn set_port_feature(&self, hub: &Hub, port: u8, feature: PortFeature) -> UsbResult<()>;

    async fn clear_port_feature(&self, hub: &Hub, port: u8, feature: PortFeature) -> UsbResult<()>;
}

pub struct HubPort {
    /// Bumped on every connect/disconnect.
    generation: AtomicU32,
    pending: AtomicU8,
    pending_speed: AtomicU8,
    device: SpinMutex<Option<Arc<Device>>>,
}

impl HubPort {
    fn new() -> Self {
        Self {
            generation: AtomicU32::new(0),
            pending: AtomicU8::new(PENDING_NONE),
            pending_speed: AtomicU8::new(0),
            device: SpinMutex::new(None),
        }
    }
}

pub struct Hub {
    pub name: String,
    pub controller: Arc<Controller>,
    pub ops: Box<dyn HubOps + Send + Sync>,
    pub ports: Vec<HubPort>,
    /// The hub's own upstream device, or [`None`] for a root hub.
    pub device: Option<Arc<Device>>,
    worker_event: Event,
    stopped: AtomicBool,
}

impl Hub {
    /// Creates a hub and spawns its enumeration worker thread.
    pub fn new(
        name: String,
        controller: Arc<Controller>,
        ops: Box<dyn HubOps + Send + Sync>,
        port_count: u8,
        device: Option<Arc<Device>>,
    ) -> EResult<Arc<Self>> {
        let mut ports = Vec::with_capacity(port_count as usize);
        for _ in 0..port_count {
            ports.push(HubPort::new());
        }

        let hub = Arc::new(Self {
            name,
            controller,
            ops,
            ports,
            device,
            worker_event: Event::new(),
            stopped: AtomicBool::new(false),
        });

        let worker = hub.clone();
        Task::run(move |_| {
            CpuData::get().scheduler.block_on(worker.worker_loop());
        })?;

        Ok(hub)
    }

    /// Creates a root hub and spawns its enumeration worker thread.
    pub fn new_root(
        name: String,
        controller: Arc<Controller>,
        ops: Box<dyn HubOps + Send + Sync>,
        port_count: u8,
    ) -> EResult<Arc<Self>> {
        Self::new(name, controller, ops, port_count, None)
    }

    /// Asks the worker to exit after draining the pending port work.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.worker_event.wake_all();
    }

    pub fn handle_connect(&self, port: u8) {
        assert!((port as usize) < self.ports.len());
        let p = &self.ports[port as usize];
        p.generation.fetch_add(1, Ordering::AcqRel);
        p.pending.store(PENDING_CONNECT, Ordering::Release);
        self.worker_event.wake_all();
    }

    pub fn handle_disconnect(&self, port: u8) {
        assert!((port as usize) < self.ports.len());
        let p = &self.ports[port as usize];
        p.generation.fetch_add(1, Ordering::AcqRel);
        p.pending.store(PENDING_DISCONNECT, Ordering::Release);
        self.worker_event.wake_all();
    }

    pub fn handle_reset(&self, port: u8, speed: Speed) {
        assert!((port as usize) < self.ports.len());
        let p = &self.ports[port as usize];
        p.pending_speed.store(speed as u8, Ordering::Release);
        p.pending.store(PENDING_RESET, Ordering::Release);
        self.worker_event.wake_all();
    }

    fn has_pending(&self) -> bool {
        self.ports
            .iter()
            .any(|p| p.pending.load(Ordering::Acquire) != PENDING_NONE)
    }

    fn is_stale(&self, port: u8, generation: u32) -> bool {
        self.ports[port as usize].generation.load(Ordering::Acquire) != generation
    }

    async fn worker_loop(self: Arc<Self>) {
        loop {
            for port in 0..self.ports.len() as u8 {
                let kind = self.ports[port as usize]
                    .pending
                    .swap(PENDING_NONE, Ordering::AcqRel);

                match kind {
                    PENDING_CONNECT => {
                        Self::teardown(&self, port).await;

                        if let Err(e) = self
                            .ops
                            .set_port_feature(self.as_ref(), port, PortFeature::PortReset)
                            .await
                        {
                            warn!("{} port {} reset failed: {:?}", self.name, port + 1, e);
                        }
                    }
                    PENDING_RESET => {
                        let speed = self.ports[port as usize]
                            .pending_speed
                            .load(Ordering::Acquire)
                            .into();
                        Self::enumerate(&self, port, speed).await;
                    }
                    PENDING_DISCONNECT => Self::teardown(&self, port).await,
                    _ => {}
                }
            }

            if self.stopped.load(Ordering::Acquire) {
                if !self.has_pending() {
                    break;
                }
                continue;
            }

            if let Some(guard) = self
                .worker_event
                .guard_if(|| !self.has_pending() && !self.stopped.load(Ordering::Acquire))
            {
                guard.wait();
            }
        }
    }

    async fn enumerate(self: &Arc<Self>, port: u8, speed: Speed) {
        let generation = self.ports[port as usize].generation.load(Ordering::Acquire);
        match self.enumerate_inner(port, speed, generation).await {
            Ok(()) => {}
            Err(e) => warn!(
                "Enumeration on {} port {} failed: {:?}",
                self.name,
                port + 1,
                e
            ),
        }
    }

    async fn enumerate_inner(
        self: &Arc<Self>,
        port: u8,
        speed: Speed,
        generation: u32,
    ) -> UsbResult<()> {
        let controller = self.controller.clone();
        let device = controller
            .ops
            .address_device(controller.as_ref(), self, port, speed)
            .await
            .inspect_err(|e| {
                warn!(
                    "{} port {}: address_device failed: {:?}",
                    self.name,
                    port + 1,
                    e
                )
            })?;

        // Read the configuration descriptor header to learn its total length.
        let mut header = [0u8; 9];
        device
            .get_descriptor(DescriptorType::Config as u8, 0, &mut header)
            .await
            .inspect_err(|e| {
                warn!(
                    "{} port {}: config header read failed: {:?}",
                    self.name,
                    port + 1,
                    e
                )
            })?;
        let total = u16::from_le_bytes([header[2], header[3]]) as usize;
        if total < header.len() || total > MAX_CONFIG_DESC_SIZE {
            warn!("Malformed config descriptor header");
            controller
                .ops
                .deaddress_device(controller.as_ref(), &device)
                .await?;
            return Err(Status::Error);
        }

        // Read the full configuration descriptor and parse it.
        let mut buf = alloc::vec![0u8; total];
        device
            .get_descriptor(DescriptorType::Config as u8, 0, &mut buf)
            .await
            .inspect_err(|e| {
                warn!(
                    "{} port {}: config body read failed: {:?}",
                    self.name,
                    port + 1,
                    e
                )
            })?;
        let configuration_value = buf[5];
        let mut interfaces = match parse_config(&buf, device.speed) {
            Ok(interfaces) => interfaces,
            Err(e) => {
                warn!(
                    "{} port {}: config descriptor parse failed: {:?}",
                    self.name,
                    port + 1,
                    e
                );
                controller
                    .ops
                    .deaddress_device(controller.as_ref(), &device)
                    .await?;
                return Err(e);
            }
        };

        if self.is_stale(port, generation) {
            controller
                .ops
                .deaddress_device(controller.as_ref(), &device)
                .await?;
            return Err(Status::Error);
        }

        device
            .set_configuration(configuration_value)
            .await
            .inspect_err(|e| {
                warn!(
                    "{} port {}: set_configuration({}) failed: {:?}",
                    self.name,
                    port + 1,
                    configuration_value,
                    e
                )
            })?;

        // Hubs carry their characteristics in the controller's device context.
        // Fetch the hub descriptor and apply it before configuring endpoints.
        let is_hub = (*device.descriptor.lock())
            .is_some_and(|d| d.device_class == spec::USB_CLASS_HUB)
            || interfaces
                .iter()
                .any(|i| i.desc.interface_class == spec::USB_CLASS_HUB);
        if is_hub {
            let marked = match fetch_hub_info(&device).await {
                Ok(info) => {
                    *device.hub_info.lock() = Some(info);
                    controller
                        .ops
                        .mark_as_hub(controller.as_ref(), &device)
                        .await
                }
                Err(e) => Err(e),
            };
            if let Err(e) = marked {
                warn!("Failed to mark device as hub: {:?}", e);
                controller
                    .ops
                    .deaddress_device(controller.as_ref(), &device)
                    .await?;
                return Err(e);
            }
        }

        for interface in &interfaces {
            for endpoint in &interface.endpoints {
                controller
                    .ops
                    .configure_ep(controller.as_ref(), &device, endpoint)
                    .await
                    .inspect_err(|e| {
                        warn!(
                            "{} port {}: configure_ep (addr {:#04x}, attr {:#04x}) failed: {:?}",
                            self.name,
                            port + 1,
                            endpoint.desc.endpoint_address,
                            endpoint.desc.attributes,
                            e
                        )
                    })?;
            }
        }

        if self.is_stale(port, generation) {
            controller
                .ops
                .deaddress_device(controller.as_ref(), &device)
                .await?;
            return Err(Status::Error);
        }

        for interface in &mut interfaces {
            device.clone().match_interface(interface);
        }

        *device.interfaces.lock() = interfaces;
        *self.ports[port as usize].device.lock() = Some(device);
        Ok(())
    }

    async fn teardown(self: &Arc<Self>, port: u8) {
        let device = self.ports[port as usize].device.lock().take();
        let Some(device) = device else {
            return;
        };

        {
            let interfaces = device.interfaces.lock();
            for interface in interfaces.iter() {
                if let Some(driver) = interface.driver {
                    let _ = (driver.detach)(device.clone(), interface);
                }
            }
        }

        let controller = self.controller.clone();
        if let Err(e) = controller
            .ops
            .deaddress_device(controller.as_ref(), &device)
            .await
        {
            warn!(
                "Deaddress on {} port {} failed: {:?}",
                self.name,
                port + 1,
                e
            );
        }
    }
}

/// Reads and parses a hub-class device's hub descriptor.
async fn fetch_hub_info(device: &Device) -> UsbResult<HubInfo> {
    let desc_type = if matches!(device.speed, Speed::Super | Speed::SuperPlus) {
        DescriptorType::SsHub
    } else {
        DescriptorType::Hub
    };

    let mut buf = [0u8; 255];
    let transferred = device
        .get_class_descriptor(desc_type as u8, 0, &mut buf)
        .await?;
    if transferred < size_of::<HubDescriptor>() || buf[1] != desc_type as u8 {
        return Err(Status::Error);
    }

    let desc = unsafe { read_unaligned(buf.as_ptr() as *const HubDescriptor) };
    if desc.nbr_ports == 0 {
        return Err(Status::Error);
    }

    let device_protocol = (*device.descriptor.lock()).map_or(0, |d| d.device_protocol);
    let power_mode = { desc.hub_characteristics } & spec::USB_HUB_POWER_SWITCHING_MODE_MASK;

    Ok(HubInfo {
        port_count: desc.nbr_ports,
        multi_tt: desc_type == DescriptorType::Hub
            && device_protocol as u32 == spec::USB_HUB_PROTOCOL_MULTI_TT,
        power_good_ms: desc.pwr_on2_pwr_good as u32 * 2,
        power_switched: power_mode & spec::USB_HUB_POWER_SWITCHING_MODE_NONE == 0,
    })
}

fn parse_config(buf: &[u8], speed: Speed) -> UsbResult<Vec<Interface>> {
    let mut interfaces: Vec<Interface> = Vec::new();
    let mut skip_endpoints = false;
    let mut drop_companion = false;
    let mut off = 0;

    while off + 2 <= buf.len() {
        let len = buf[off] as usize;
        if len < 2 || off + len > buf.len() {
            break;
        }
        let dtype = buf[off + 1];

        if dtype == DescriptorType::Interface as u8 {
            if len < size_of::<InterfaceDescriptor>() {
                warn!("Malformed interface descriptor (len {})", len);
                return Err(Status::Error);
            }
            // TODO: zerocopy here?
            let desc = unsafe { read_unaligned(buf[off..].as_ptr() as *const InterfaceDescriptor) };
            drop_companion = false;
            if desc.alternate_setting != 0 {
                skip_endpoints = true;
            } else {
                skip_endpoints = false;
                interfaces.push(Interface {
                    desc,
                    endpoints: Vec::new(),
                    driver: None,
                    driver_data: SpinMutex::new(None),
                });
            }
        } else if dtype == DescriptorType::Endpoint as u8 && !skip_endpoints {
            if len < size_of::<EndpointDescriptor>() {
                warn!("Skipping malformed endpoint descriptor (len {})", len);
                drop_companion = true;
            } else {
                // TODO: zerocopy here?
                let desc =
                    unsafe { read_unaligned(buf[off..].as_ptr() as *const EndpointDescriptor) };
                if desc.valid_for_speed(speed) {
                    drop_companion = false;
                    if let Some(interface) = interfaces.last_mut() {
                        interface.endpoints.push(Endpoint {
                            desc,
                            ss_companion: None,
                        });
                    }
                } else {
                    // Unsupported or zero-bandwidth endpoint.
                    warn!(
                        "Skipping unsupported endpoint (addr {:#04x}, attr {:#04x}, interval {}, mps {}) at speed {:?}",
                        desc.endpoint_address,
                        desc.attributes,
                        desc.interval,
                        desc.max_packet_size(),
                        speed
                    );
                    drop_companion = true;
                }
            }
        } else if dtype == DescriptorType::SsEpCompanion as u8 && !skip_endpoints {
            if len < size_of::<SsEpCompanionDescriptor>() {
                warn!(
                    "Skipping malformed endpoint companion descriptor (len {})",
                    len
                );
            } else if !drop_companion {
                // TODO: use zerocopy here?
                let companion = unsafe {
                    read_unaligned(buf[off..].as_ptr() as *const SsEpCompanionDescriptor)
                };
                if let Some(endpoint) = interfaces.last_mut().and_then(|i| i.endpoints.last_mut()) {
                    endpoint.ss_companion = Some(companion);
                }
            }
        }

        off += len;
    }

    // Super speed endpoints require a companion descriptor.
    if matches!(speed, Speed::Super | Speed::SuperPlus) {
        for interface in &mut interfaces {
            interface.endpoints.retain(|ep| {
                if ep.ss_companion.is_none() {
                    warn!("Dropping super-speed endpoint without SS companion");
                    false
                } else {
                    true
                }
            });
        }
    }

    Ok(interfaces)
}
