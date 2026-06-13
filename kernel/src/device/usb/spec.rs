use crate::device::usb::Speed;

#[repr(packed, C)]
pub struct DescriptorHeader {
    pub length: u8,
    pub descriptor_type: DescriptorType,
}

#[repr(u8)]
#[derive(PartialEq)]
pub enum DescriptorType {
    Device = 1,
    Config = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
    SsEpCompanion = 48,
    Hub = 0x29,
    SsHub = 0x2A,
}

#[repr(packed, C)]
pub struct DeviceDescriptor {
    pub header: DescriptorHeader,
    pub usb: u16,
    pub device_class: u8,
    pub device_sub_class: u8,
    pub device_protocol: u8,
    pub max_packet_size_ep0: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device: u16,
    pub manufacturer: u8,
    pub product: u8,
    pub serial_number: u8,
    pub num_configurations: u8,
}

impl DeviceDescriptor {
    pub fn is_valid(&self, speed: Speed, transferred: usize) -> bool {
        transferred >= size_of::<Self>()
            && self.header.length as usize == size_of::<Self>()
            && self.header.descriptor_type == DescriptorType::Device
            && self.num_configurations > 0
            && match speed {
                Speed::Super | Speed::SuperPlus => self.max_packet_size_ep0 == 9,
                Speed::Low => self.max_packet_size_ep0 == 8,
                Speed::High => self.max_packet_size_ep0 == 64,
                Speed::Full => matches!(self.max_packet_size_ep0, 8 | 16 | 32 | 64),
                _ => false,
            }
    }
}

#[repr(packed, C)]
pub struct ConfigDescriptor {
    pub header: DescriptorHeader,
    pub total_length: u16,
    pub num_interfaces: u8,
    pub configuration_value: u8,
    pub configuration: u8,
    pub attributes: u8,
    pub max_power: u8,
}

#[repr(packed, C)]
pub struct InterfaceDescriptor {
    pub header: DescriptorHeader,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub num_endpoints: u8,
    pub interface_class: u8,
    pub interface_sub_class: u8,
    pub interface_protocol: u8,
    pub interface: u8,
}

pub const USB_ENDPOINT_ADDRESS_NUM_MASK: u32 = 0x0f;
pub const USB_ENDPOINT_ADDRESS_DIR_IN: u32 = 0x80;

pub enum EndpointAttribute {}

pub const USB_ENDPOINT_ATTRIB_TYPE_MASK: u32 = 0x03;
pub const USB_ENDPOINT_ATTRIB_TYPE_CONTROL: u32 = 0x00;
pub const USB_ENDPOINT_ATTRIB_TYPE_ISOCH: u32 = 0x01;
pub const USB_ENDPOINT_ATTRIB_TYPE_BULK: u32 = 0x02;
pub const USB_ENDPOINT_ATTRIB_TYPE_INTR: u32 = 0x03;

#[repr(packed, C)]
pub struct EndpointDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub endpoint_address: u8,
    pub attributes: u8,
    max_packet_size: u16,
    pub interval: u8,
}

impl EndpointDescriptor {
    const MAX_PACKET_SIZE_MASK: u16 = 0x7ff;

    pub const fn max_packet_size(&self) -> u16 {
        self.max_packet_size & Self::MAX_PACKET_SIZE_MASK
    }
}

#[repr(packed, C)]
pub struct SsEpCompanionDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub max_burst: u8,
    pub attributes: u8,
    pub bytes_per_interval: u16,
}

pub const USB_HUB_PROTOCOL_MULTI_TT: u32 = 2;

#[repr(packed, C)]
pub struct HubDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub nbr_ports: u8,
    pub hub_characteristics: u16,
    pub pwr_on2_pwr_good: u8,
    pub hub_contr_current: u8,
}

#[repr(packed, C)]
pub struct SsHubDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub nbr_ports: u8,
    pub hub_characteristics: u16,
    pub pwr_on2_pwr_good: u8,
    pub hub_contr_current: u8,
    pub hub_hdr_dec_lat: u8,
    pub hub_delay: u16,
}

pub const USB_REQUEST_RECIP_DEVICE: u32 = 0x00;
pub const USB_REQUEST_RECIP_INTERFACE: u32 = 0x01;
pub const USB_REQUEST_RECIP_ENDPOINT: u32 = 0x02;
pub const USB_REQUEST_RECIP_OTHER: u32 = 0x03;

pub const USB_REQUEST_STANDARD: u32 = 0x00;
pub const USB_REQUEST_CLASS: u32 = 0x20;
pub const USB_REQUEST_VENDOR: u32 = 0x40;

pub const USB_REQUEST_DIR_TO_DEVICE: u32 = 0x00;
pub const USB_REQUEST_DIR_TO_HOST: u32 = 0x80;

pub const USB_REQUEST_GET_DESCRIPTOR: u32 = 6;
pub const USB_REQUEST_SET_CONFIGURATION: u32 = 9;
pub const USB_REQUEST_SET_INTERFACE: u32 = 11;

#[repr(packed, C)]
#[derive(Clone, Copy)]
pub struct Setup {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

bitflags::bitflags! {
    pub struct PortStatus: u16 {
        const PortConnection = 1 << 0;
        const PortEnable = 1 << 1;
        const PortOverCurrent = 1 << 3;
        const PortReset = 1 << 4;
        const PortPower = 1 << 8;
        const LowSpeed = 1 << 9;
        const HighSpeed = 1 << 10;
    }

    pub struct PortChange: u16 {
        const PortConnection = 1 << 0;
        const PortEnable = 1 << 1;
        const PortOverCurrent = 1 << 3;
        const PortReset = 1 << 4;
    }
}

#[repr(u16)]
pub enum PortFeature {
    PortConnection = 0,
    PortEnable = 1,
    PortReset = 4,
    PortPower = 8,
    PortLowSpeed = 9,
    CPortConnection = 16,
    CPortEnable = 17,
    CPortOverCurrent = 19,
    CPortReset = 20,
}
