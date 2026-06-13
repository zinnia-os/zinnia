use crate::device::usb::{
    Controller, UsbResult,
    spec::{PortChange, PortFeature, PortStatus},
};
use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use async_trait::async_trait;

#[async_trait]
pub trait HubOps {
    async fn get_port_status(&self, hub: &Hub, port: u8) -> UsbResult<(PortStatus, PortChange)>;

    async fn set_port_feature(&self, hub: &Hub, feature: PortFeature) -> UsbResult<()>;

    async fn clear_port_feature(&self, hub: &Hub, port: u8, feature: PortFeature) -> UsbResult<()>;
}

pub struct HubPort {}

pub struct Hub {
    pub name: String,
    pub controller: Arc<Controller>,
    pub ops: Box<dyn HubOps>,
    pub ports: Vec<HubPort>,
}

impl Hub {
    pub fn handle_connect(&self, port: u8) {
        assert!((port as usize) < self.ports.len());

        todo!()
    }

    pub fn handle_disconnect(&self, port: u8) {
        todo!()
    }

    pub fn handle_reset(&self, port: u8, speed: super::Speed) {
        todo!()
    }
}
