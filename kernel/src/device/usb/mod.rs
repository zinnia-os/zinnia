use crate::{
    device::usb::hub::Hub,
    memory::{IovecIter, PhysAddr},
    posix::errno::EResult,
    util::mutex::spin::SpinMutex,
};
use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use async_trait::async_trait;
use core::any::Any;
use num_enum::FromPrimitive;

pub mod hid;
pub mod hub;
pub mod hub_class;
pub mod spec;
pub mod storage;

pub type UsbResult<T> = Result<T, Status>;

bitflags::bitflags! {
    pub struct TransferFlags: u32 {
        const ToDevice = 1 << 0;
        const ToHost = 1 << 1;
        const BufferPhysical = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    Control,
    Bulk,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    ShortPacket,
    Error,
    Stall,
    FlowError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive)]
#[repr(u8)]
pub enum Speed {
    #[default]
    Unknown,
    Low,
    Full,
    High,
    Super,
    SuperPlus,
}

pub struct Endpoint {
    pub desc: spec::EndpointDescriptor,
    /// SuperSpeed endpoint companion descriptor.
    pub ss_companion: Option<spec::SsEpCompanionDescriptor>,
}

pub struct Interface {
    pub desc: spec::InterfaceDescriptor,
    pub endpoints: Vec<Endpoint>,
    pub driver: Option<&'static Driver>,
    /// Private state of the attached class driver, cleared by its `detach`.
    pub driver_data: SpinMutex<Option<Arc<dyn Any + Send + Sync>>>,
}

/// Parsed hub descriptor of a hub-class device.
#[derive(Clone, Copy)]
pub struct HubInfo {
    pub port_count: u8,
    /// USB 2.0 hub with one transaction translator per port.
    pub multi_tt: bool,
    /// Time from port power-on until power is stable, in milliseconds.
    pub power_good_ms: u32,
    /// Whether the hub requires per-port power switching.
    pub power_switched: bool,
}

/// Per controller operations the host controller driver must implement.
#[async_trait(?Send)]
pub trait ControllerOps {
    async fn address_device(
        &self,
        controller: &Controller,
        hub: &Arc<Hub>,
        port: u8,
        speed: Speed,
    ) -> UsbResult<Arc<Device>>;

    async fn deaddress_device(&self, controller: &Controller, device: &Device) -> UsbResult<()>;

    async fn mark_as_hub(&self, controller: &Controller, device: &Device) -> UsbResult<()>;

    async fn configure_ep(
        &self,
        controller: &Controller,
        device: &Device,
        endpoint: &Endpoint,
    ) -> UsbResult<()>;

    /// Submits a transfer and returns the number of bytes transferred.
    async fn transfer(
        &self,
        controller: &Controller,
        device: &Device,
        transfer: &mut Transfer,
    ) -> UsbResult<usize>;
}

pub struct Controller {
    pub ops: Box<dyn ControllerOps + Send + Sync>,
}

pub struct Transfer<'a, 'b> {
    /// The endpoint this transfer targets, or [`None`] for a control transfer on EP0.
    pub endpoint: Option<&'a Endpoint>,
    pub flags: TransferFlags,
    pub typ: TransferType,
    pub device: Option<&'a Device>,

    pub setup: Option<spec::Setup>,
    pub data: &'a mut IovecIter<'b>,
    /// A contiguous physical buffer to DMA directly, bypassing `data`. The host
    /// controller uses this instead of bouncing through `data` when set.
    pub data_phys: Option<(PhysAddr, usize)>,
}

pub struct Driver {
    pub name: &'static str,
    pub probe: fn(device: Arc<Device>, interface: &Interface) -> EResult<()>,
    pub attach: fn(device: Arc<Device>, interface: &Interface) -> EResult<()>,
    pub detach: fn(device: Arc<Device>, interface: &Interface) -> EResult<()>,
}

impl Driver {
    pub fn register(&'static self) {
        DRIVERS.lock().push(self);
    }
}

static DRIVERS: SpinMutex<Vec<&'static Driver>> = SpinMutex::new(Vec::new());

pub struct Device {
    pub parent_hub: Arc<Hub>,
    /// 1-based port on `parent_hub` this device is attached to.
    pub port: u8,
    pub speed: Speed,
    /// The device descriptor, read while the device is addressed.
    pub descriptor: SpinMutex<Option<spec::DeviceDescriptor>>,
    /// Hub characteristics if this device is a hub.
    pub hub_info: SpinMutex<Option<HubInfo>>,
    pub driver_data: SpinMutex<Option<Arc<dyn Any + Send + Sync>>>,
    /// The interfaces of the active configuration.
    pub interfaces: SpinMutex<Vec<Interface>>,
}

impl Device {
    pub fn new(parent_hub: Arc<Hub>, port: u8, speed: Speed) -> Self {
        Self {
            parent_hub,
            port,
            speed,
            descriptor: SpinMutex::new(None),
            hub_info: SpinMutex::new(None),
            driver_data: SpinMutex::new(None),
            interfaces: SpinMutex::new(Vec::new()),
        }
    }

    /// Finds the first registered driver whose `probe` accepts `interface` and attaches it.
    pub fn match_interface(self: Arc<Self>, interface: &mut Interface) {
        let drivers: Vec<_> = DRIVERS.lock().iter().copied().collect();

        for driver in drivers {
            if (driver.probe)(self.clone(), interface).is_err() {
                continue;
            }

            match (driver.attach)(self.clone(), interface) {
                Ok(()) => {
                    interface.driver = Some(driver);
                    log!(
                        "Interface {} attached to driver '{}'",
                        { interface.desc.interface_number },
                        driver.name,
                    );
                    return;
                }
                Err(e) => warn!(
                    "Driver \"{}\" failed to attach interface {}: {:?}",
                    driver.name,
                    { interface.desc.interface_number },
                    e,
                ),
            }
        }

        log!("No driver found for interface {}", {
            interface.desc.interface_number
        });
    }

    pub async fn submit(&self, mut transfer: Transfer<'_, '_>) -> UsbResult<usize> {
        let controller = self.parent_hub.controller.as_ref();
        controller
            .ops
            .transfer(controller, self, &mut transfer)
            .await
    }

    /// Returns the number of bytes transferred.
    pub async fn control_transfer(&self, setup: spec::Setup, data: &mut [u8]) -> UsbResult<usize> {
        let to_host = setup.request_type & spec::USB_REQUEST_DIR_TO_HOST as u8 != 0;
        let flags = if to_host {
            TransferFlags::ToHost
        } else {
            TransferFlags::ToDevice
        };

        // SAFETY: `data` outlives the iterator below.
        let iovec = unsafe { IovecIter::iovec_from_mut_ptr(data) };
        let iovecs = [iovec];
        let mut iter = unsafe { IovecIter::new_kernel(&iovecs) };

        let mut transfer = Transfer {
            endpoint: None,
            flags,
            typ: TransferType::Control,
            device: Some(self),
            setup: Some(setup),
            data: &mut iter,
            data_phys: None,
        };

        let controller = self.parent_hub.controller.as_ref();
        controller
            .ops
            .transfer(controller, self, &mut transfer)
            .await
    }

    pub async fn bulk_transfer(
        &self,
        endpoint: &Endpoint,
        buf: &mut [u8],
        to_host: bool,
    ) -> UsbResult<usize> {
        let flags = if to_host {
            TransferFlags::ToHost
        } else {
            TransferFlags::ToDevice
        };

        // SAFETY: `buf` outlives the iterator below.
        let iovec = unsafe { IovecIter::iovec_from_mut_ptr(buf) };
        let iovecs = [iovec];
        let mut iter = unsafe { IovecIter::new_kernel(&iovecs) };

        let mut transfer = Transfer {
            endpoint: Some(endpoint),
            flags,
            typ: TransferType::Bulk,
            device: Some(self),
            setup: None,
            data: &mut iter,
            data_phys: None,
        };

        let controller = self.parent_hub.controller.as_ref();
        controller
            .ops
            .transfer(controller, self, &mut transfer)
            .await
    }

    /// Bulk transfer that DMAs directly from/into a contiguous physical buffer,
    /// skipping the controller's bounce copy. Used for block I/O.
    pub async fn bulk_transfer_phys(
        &self,
        endpoint: &Endpoint,
        phys: PhysAddr,
        len: usize,
        to_host: bool,
    ) -> UsbResult<usize> {
        let flags = TransferFlags::BufferPhysical
            | if to_host {
                TransferFlags::ToHost
            } else {
                TransferFlags::ToDevice
            };

        let mut iter = unsafe { IovecIter::new_kernel(&[]) };
        let mut transfer = Transfer {
            endpoint: Some(endpoint),
            flags,
            typ: TransferType::Bulk,
            device: Some(self),
            setup: None,
            data: &mut iter,
            data_phys: Some((phys, len)),
        };

        let controller = self.parent_hub.controller.as_ref();
        controller
            .ops
            .transfer(controller, self, &mut transfer)
            .await
    }

    pub async fn interrupt_transfer(
        &self,
        endpoint: &Endpoint,
        buf: &mut [u8],
    ) -> UsbResult<usize> {
        // SAFETY: `buf` outlives the iterator below.
        let iovec = unsafe { IovecIter::iovec_from_mut_ptr(buf) };
        let iovecs = [iovec];
        let mut iter = unsafe { IovecIter::new_kernel(&iovecs) };

        let mut transfer = Transfer {
            endpoint: Some(endpoint),
            flags: TransferFlags::ToHost,
            typ: TransferType::Interrupt,
            device: Some(self),
            setup: None,
            data: &mut iter,
            data_phys: None,
        };

        let controller = self.parent_hub.controller.as_ref();
        controller
            .ops
            .transfer(controller, self, &mut transfer)
            .await
    }

    pub async fn get_descriptor(
        &self,
        desc_type: u8,
        index: u8,
        buf: &mut [u8],
    ) -> UsbResult<usize> {
        let setup = spec::Setup {
            request_type: spec::USB_REQUEST_DIR_TO_HOST as u8,
            request: spec::USB_REQUEST_GET_DESCRIPTOR as u8,
            value: ((desc_type as u16) << 8) | index as u16,
            index: 0,
            length: buf.len() as u16,
        };

        self.control_transfer(setup, buf).await
    }

    pub async fn get_class_descriptor(
        &self,
        desc_type: u8,
        index: u8,
        buf: &mut [u8],
    ) -> UsbResult<usize> {
        let setup = spec::Setup {
            request_type: (spec::USB_REQUEST_DIR_TO_HOST | spec::USB_REQUEST_CLASS) as u8,
            request: spec::USB_REQUEST_GET_DESCRIPTOR as u8,
            value: ((desc_type as u16) << 8) | index as u16,
            index: 0,
            length: buf.len() as u16,
        };

        self.control_transfer(setup, buf).await
    }

    /// Reads and UTF-16-decodes the string descriptor at `index`, or [`None`] if
    /// `index` is 0 or the device returns no usable string.
    pub async fn get_string(&self, index: u8) -> Option<String> {
        if index == 0 {
            return None;
        }

        // String descriptor 0 holds the supported language ids; default to English.
        let mut langs = [0u8; 4];
        let langid = match self.get_string_raw(0, 0, &mut langs).await {
            Ok(n) if n >= 4 => u16::from_le_bytes([langs[2], langs[3]]),
            _ => 0x0409,
        };

        let mut buf = [0u8; 255];
        let n = self.get_string_raw(index, langid, &mut buf).await.ok()?;
        if n < 2 || buf[1] != spec::DescriptorType::String as u8 {
            return None;
        }

        let len = (buf[0] as usize).min(n);
        let units = buf[2..len]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]));
        let s: String = char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{fffd}'))
            .collect();
        let s = String::from(s.trim());
        if s.is_empty() { None } else { Some(s) }
    }

    async fn get_string_raw(&self, index: u8, langid: u16, buf: &mut [u8]) -> UsbResult<usize> {
        let setup = spec::Setup {
            request_type: spec::USB_REQUEST_DIR_TO_HOST as u8,
            request: spec::USB_REQUEST_GET_DESCRIPTOR as u8,
            value: ((spec::DescriptorType::String as u16) << 8) | index as u16,
            index: langid,
            length: buf.len() as u16,
        };
        self.control_transfer(setup, buf).await
    }

    pub async fn set_configuration(&self, configuration: u8) -> UsbResult<()> {
        let setup = spec::Setup {
            request_type: spec::USB_REQUEST_DIR_TO_DEVICE as u8,
            request: spec::USB_REQUEST_SET_CONFIGURATION as u8,
            value: configuration as u16,
            index: 0,
            length: 0,
        };

        self.control_transfer(setup, &mut []).await.map(|_| ())
    }
}
