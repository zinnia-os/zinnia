use crate::{XhciController, spec::port};
use zinnia::{
    alloc::{boxed::Box, sync::Arc},
    async_trait::async_trait,
    device::usb::{
        Status, UsbResult,
        hub::{Hub, HubOps},
        spec::{PortChange, PortFeature, PortStatus},
    },
};

pub struct XhciRootHubOps {
    pub ctrl: Arc<XhciController>,
}

#[async_trait(?Send)]
impl HubOps for XhciRootHubOps {
    async fn get_port_status(&self, _hub: &Hub, port: u8) -> UsbResult<(PortStatus, PortChange)> {
        let portsc = self.ctrl.port_rd(port as usize, port::PORTSC);

        let mut status = PortStatus::empty();
        if portsc.read_field(port::portsc::CCS).value() != 0 {
            status |= PortStatus::PortConnection;
        }
        if portsc.read_field(port::portsc::PED).value() != 0 {
            status |= PortStatus::PortEnable;
        }
        if portsc.read_field(port::portsc::OCA).value() != 0 {
            status |= PortStatus::PortOverCurrent;
        }
        if portsc.read_field(port::portsc::PR).value() != 0 {
            status |= PortStatus::PortReset;
        }
        if portsc.read_field(port::portsc::PP).value() != 0 {
            status |= PortStatus::PortPower;
        }
        match portsc.read_field(port::portsc::SPEED).value() {
            2 => status |= PortStatus::LowSpeed,
            3 => status |= PortStatus::HighSpeed,
            _ => {}
        }

        let mut change = PortChange::empty();
        if portsc.read_field(port::portsc::CSC).value() != 0 {
            change |= PortChange::PortConnection;
        }
        if portsc.read_field(port::portsc::PEC).value() != 0 {
            change |= PortChange::PortEnable;
        }
        if portsc.read_field(port::portsc::OCC).value() != 0 {
            change |= PortChange::PortOverCurrent;
        }
        if portsc.read_field(port::portsc::PRC).value() != 0 {
            change |= PortChange::PortReset;
        }

        Ok((status, change))
    }

    async fn set_port_feature(&self, _hub: &Hub, port: u8, feature: PortFeature) -> UsbResult<()> {
        match feature {
            PortFeature::PortReset => self.ctrl.port_set_reset(port as usize),
            PortFeature::PortPower => self.ctrl.port_set_power(port as usize),
            _ => return Err(Status::Error),
        }
        Ok(())
    }

    async fn clear_port_feature(
        &self,
        _hub: &Hub,
        port: u8,
        feature: PortFeature,
    ) -> UsbResult<()> {
        let bit = match feature {
            PortFeature::CPortConnection => port::portsc_bits::CSC,
            PortFeature::CPortEnable => port::portsc_bits::PEC,
            PortFeature::CPortOverCurrent => port::portsc_bits::OCC,
            PortFeature::CPortReset => port::portsc_bits::PRC,
            _ => return Err(Status::Error),
        };
        self.ctrl.port_ack_change(port as usize, bit);
        Ok(())
    }
}
