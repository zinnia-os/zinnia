use crate::{
    device::net::{
        interface::ManagedInterface,
        l3::ipv4::{self, Ipv4Addr, Ipv4Endpoint, Ipv4Header, Ipv4Protocol},
        l4,
    },
    posix::errno::{EResult, Errno},
};
use alloc::vec;

pub const ICMP_HEADER_LEN: usize = 8;

const ICMP_ECHO_REPLY: u8 = 0;
const ICMP_ECHO_REQUEST: u8 = 8;
const ICMP_DEST_UNREACHABLE: u8 = 3;

const ICMP_NET_UNREACHABLE: u8 = 0;
const ICMP_HOST_UNREACHABLE: u8 = 1;
const ICMP_PROTOCOL_UNREACHABLE: u8 = 2;
const ICMP_PORT_UNREACHABLE: u8 = 3;

const QUOTED_PAYLOAD_LEN: usize = 8;

const IPV4_MIN_HEADER_LEN: usize = 20;

pub struct EchoRequest<'a> {
    body: &'a [u8],
}

impl<'a> EchoRequest<'a> {
    pub fn parse(packet: &'a [u8]) -> Option<Self> {
        if packet.len() < ICMP_HEADER_LEN {
            return None;
        }
        if packet[0] != ICMP_ECHO_REQUEST || packet[1] != 0 {
            return None;
        }
        if ipv4::checksum(packet) != 0 {
            return None;
        }

        Some(Self { body: packet })
    }

    pub fn len(&self) -> usize {
        self.body.len()
    }

    pub fn write_reply(&self, packet: &mut [u8]) -> EResult<()> {
        if packet.len() < self.body.len() {
            return Err(Errno::EINVAL);
        }

        packet[..self.body.len()].copy_from_slice(self.body);
        packet[0] = ICMP_ECHO_REPLY;
        packet[2..4].copy_from_slice(&0u16.to_be_bytes());

        let sum = ipv4::checksum(&packet[..self.body.len()]);
        packet[2..4].copy_from_slice(&sum.to_be_bytes());
        Ok(())
    }
}

pub fn process_dest_unreachable(packet: &[u8]) -> bool {
    if packet.len() < ICMP_HEADER_LEN || packet[0] != ICMP_DEST_UNREACHABLE {
        return false;
    }
    if ipv4::checksum(packet) != 0 {
        return false;
    }

    let error = match packet[1] {
        ICMP_PORT_UNREACHABLE => Errno::ECONNREFUSED,
        ICMP_NET_UNREACHABLE => Errno::ENETUNREACH,
        ICMP_HOST_UNREACHABLE => Errno::EHOSTUNREACH,
        ICMP_PROTOCOL_UNREACHABLE => Errno::ENOPROTOOPT,
        _ => Errno::EHOSTUNREACH,
    };

    let quoted = &packet[ICMP_HEADER_LEN..];
    if quoted.len() < IPV4_MIN_HEADER_LEN {
        return false;
    }
    let quoted_header_len = ((quoted[0] & 0x0f) as usize) * 4;
    if quoted[0] >> 4 != 4 || quoted_header_len < IPV4_MIN_HEADER_LEN {
        return false;
    }
    if Ipv4Protocol::from_u8(quoted[9]) != Ipv4Protocol::Udp {
        return false;
    }

    let Some(udp) = quoted.get(quoted_header_len..quoted_header_len + 4) else {
        return false;
    };

    let local = Ipv4Endpoint {
        addr: Ipv4Addr::new([quoted[12], quoted[13], quoted[14], quoted[15]]),
        port: u16::from_be_bytes([udp[0], udp[1]]),
    };
    let remote = Ipv4Endpoint {
        addr: Ipv4Addr::new([quoted[16], quoted[17], quoted[18], quoted[19]]),
        port: u16::from_be_bytes([udp[2], udp[3]]),
    };

    l4::udp::deliver_error(local, remote, error);
    true
}

pub fn send_port_unreachable(interface: &ManagedInterface, offending: &Ipv4Header<'_>) {
    let destination = offending.source();
    if destination == Ipv4Addr::ANY
        || destination == Ipv4Addr::BROADCAST
        || offending.destination() == Ipv4Addr::BROADCAST
        || offending.destination() == interface.broadcast_ipv4()
    {
        return;
    }

    let header = offending.header_bytes();
    let payload = offending.payload();
    let quoted_len = QUOTED_PAYLOAD_LEN.min(payload.len());

    let mut packet = vec![0u8; ICMP_HEADER_LEN + header.len() + quoted_len];
    packet[0] = ICMP_DEST_UNREACHABLE;
    packet[1] = ICMP_PORT_UNREACHABLE;
    packet[ICMP_HEADER_LEN..ICMP_HEADER_LEN + header.len()].copy_from_slice(header);
    packet[ICMP_HEADER_LEN + header.len()..].copy_from_slice(&payload[..quoted_len]);

    let sum = ipv4::checksum(&packet);
    packet[2..4].copy_from_slice(&sum.to_be_bytes());

    let _ = ipv4::send_packet(interface, destination, Ipv4Protocol::Icmp, &packet);
}
