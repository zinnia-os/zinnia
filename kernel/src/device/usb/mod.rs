use crate::{device::usb::hub::Hub, memory::IovecIter, posix::errno::EResult};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use async_trait::async_trait;

pub mod hub;
pub mod spec;

pub type UsbResult<T> = Result<T, Status>;

bitflags::bitflags! {
    pub struct TransferFlags: u32 {
        const ToDevice = 1 << 0;
        const ToHost = 1 << 1;
        const BufferPhysical = 1 << 2;
    }
}

pub enum TransferType {
    Control,
    Bulk,
    Interrupt,
}

pub enum Status {
    ShortPacket,
    Error,
    Stall,
    FlowError,
}

pub enum Speed {
    Unknown,
    Low,
    Full,
    High,
    Super,
    SuperPlus,
}

pub struct Endpoint {
    pub desc: spec::EndpointDescriptor,
    pub ss_companion: spec::SsEpCompanionDescriptor,
}

pub struct Interface {
    pub desc: spec::InterfaceDescriptor,
    pub endpoints: Vec<Endpoint>,
    pub driver: &'static Driver,
}

#[async_trait]
pub trait ControllerOps {
    async fn address_device(
        &self,
        controller: &Controller,
        hub: &Hub,
        speed: Speed,
    ) -> UsbResult<()>;

    async fn deaddress_device(&self, controller: &Controller, device: &Device) -> UsbResult<()>;

    async fn mark_as_hub(&self, controller: &Controller, device: &Device) -> UsbResult<()>;

    async fn configure_ep(
        &self,
        controller: &Controller,
        device: &Device,
        endpoint: &Endpoint,
    ) -> UsbResult<()>;

    async fn transfer(
        &self,
        controller: &Controller,
        device: &Device,
        transfer: &Transfer,
    ) -> UsbResult<()>;
}

pub struct Controller {
    pub ops: Box<dyn ControllerOps>,
}

pub struct Transfer<'a, 'b> {
    pub endpoint: &'a Endpoint,
    pub flags: TransferFlags,
    pub typ: TransferType,
    pub device: Option<&'a Device>,

    pub setup: Option<spec::Setup>,
    pub data: &'a IovecIter<'b>,
}

pub struct Device {
    pub parent_hub: Arc<Hub>,
}

impl Device {
    pub async fn submit(&self, transfer: Transfer<'_, '_>) -> UsbResult<()> {
        let controller = self.parent_hub.controller.as_ref();
        controller.ops.transfer(controller, self, &transfer).await
    }
}

pub struct Driver {
    pub name: &'static str,
    pub probe: fn(device: &Device, interface: &Interface) -> EResult<()>,
    pub attach: fn(device: &Device, interface: &Interface) -> EResult<()>,
    pub detach: fn(device: &Device, interface: &Interface) -> EResult<()>,
}

pub enum DriverScore {
    None = 0,
    Generic = 10,
    ClassMatch = 30,
    SubclassMatch = 50,
    ProtocolMatch = 60,
    VendorMatch = 80,
    ProductMatch = 90,
    ExactMatch = 100,
}
