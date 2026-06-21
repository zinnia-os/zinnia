use crate::{
    device::net::{
        interface::{MAX_ETH_PAYLOAD_LEN, MAX_IPV4_PAYLOAD_LEN, ManagedInterface},
        l2::{
            eth::{EthHeader, EtherType},
            mac::MacAddr,
        },
        l3::icmp::EchoRequest,
        l4,
    },
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};
use alloc::vec::Vec;
use core::fmt::{Display, Formatter};

pub const IPV4_HEADER_LEN: usize = 20;
const PENDING_ARP_LIMIT: usize = 32;

static PENDING_ARP: SpinMutex<Vec<PendingArpPacket>> = SpinMutex::new(Vec::new());

struct PendingArpPacket {
    interface: usize,
    next_hop: Ipv4Addr,
    destination: Ipv4Addr,
    protocol: Ipv4Protocol,
    len: usize,
    payload: [u8; MAX_IPV4_PAYLOAD_LEN],
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv4Addr {
    buf: [u8; 4],
}

impl Ipv4Addr {
    pub const ANY: Self = Self { buf: [0; 4] };
    pub const BROADCAST: Self = Self { buf: [0xff; 4] };

    pub const fn new(bytes: [u8; 4]) -> Self {
        Self { buf: bytes }
    }

    pub const fn from_u32(v: u32) -> Self {
        Self {
            buf: v.to_be_bytes(),
        }
    }

    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.buf
    }

    pub const fn as_u32(self) -> u32 {
        u32::from_be_bytes(self.buf)
    }
}

impl Ipv4Addr {
    /// Parse a string representation like `10.0.2.15`.
    pub fn parse(s: &str) -> Option<Self> {
        let mut buf = [0u8; 4];
        let mut parts = s.split('.');
        for byte in &mut buf {
            *byte = parts.next()?.parse::<u8>().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Self { buf })
    }
}

impl Display for Ipv4Addr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "{}.{}.{}.{}",
            self.buf[0], self.buf[1], self.buf[2], self.buf[3]
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ipv4Protocol {
    Icmp,
    Tcp,
    Udp,
    Other(u8),
}

impl Ipv4Protocol {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Icmp,
            6 => Self::Tcp,
            17 => Self::Udp,
            _ => Self::Other(v),
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::Icmp => 1,
            Self::Tcp => 6,
            Self::Udp => 17,
            Self::Other(v) => v,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv4Endpoint {
    pub addr: Ipv4Addr,
    pub port: u16,
}

pub struct Ipv4Header<'a> {
    packet: &'a [u8],
    header_len: usize,
    total_len: usize,
}

impl<'a> Ipv4Header<'a> {
    pub fn parse(packet: &'a [u8]) -> Option<Self> {
        if packet.len() < IPV4_HEADER_LEN {
            return None;
        }

        let version = packet[0] >> 4;
        let header_len = ((packet[0] & 0x0f) as usize) * 4;
        if version != 4 || header_len < IPV4_HEADER_LEN || packet.len() < header_len {
            return None;
        }

        let total_len = u16::from_be_bytes([packet[2], packet[3]]) as usize;
        if total_len < header_len || total_len > packet.len() {
            return None;
        }

        if checksum(&packet[..header_len]) != 0 {
            return None;
        }

        Some(Self {
            packet,
            header_len,
            total_len,
        })
    }

    pub fn source(&self) -> Ipv4Addr {
        Ipv4Addr::new([
            self.packet[12],
            self.packet[13],
            self.packet[14],
            self.packet[15],
        ])
    }

    pub fn destination(&self) -> Ipv4Addr {
        Ipv4Addr::new([
            self.packet[16],
            self.packet[17],
            self.packet[18],
            self.packet[19],
        ])
    }

    pub fn protocol(&self) -> Ipv4Protocol {
        Ipv4Protocol::from_u8(self.packet[9])
    }

    pub fn is_fragmented(&self) -> bool {
        let flags_fragment = u16::from_be_bytes([self.packet[6], self.packet[7]]);
        flags_fragment & 0x3fff != 0
    }

    pub fn payload(&self) -> &'a [u8] {
        &self.packet[self.header_len..self.total_len]
    }
}

pub fn process_packet(interface: &ManagedInterface, eth: &EthHeader<'_>) -> EResult<bool> {
    let Some(header) = Ipv4Header::parse(eth.payload()) else {
        return Ok(false);
    };
    let dst = header.destination();
    if dst != interface.ip() && dst != Ipv4Addr::BROADCAST && dst != interface.broadcast_ipv4() {
        return Ok(false);
    }
    if header.is_fragmented() {
        return Ok(false);
    }

    interface.arp_cache().insert(header.source(), eth.src());

    crate::device::net::l3::raw::deliver(header.protocol(), eth.payload());

    match header.protocol() {
        Ipv4Protocol::Icmp => process_icmp(interface, eth.src(), &header),
        Ipv4Protocol::Tcp => l4::tcp::process_packet(interface, &header),
        Ipv4Protocol::Udp => l4::udp::process_packet(interface, &header),
        _ => Ok(false),
    }
}

/// Implements a layer 4 protocol for IPv4.
pub trait Ipv4Transport {}

fn process_icmp(
    interface: &ManagedInterface,
    dst_mac: MacAddr,
    header: &Ipv4Header<'_>,
) -> EResult<bool> {
    let Some(request) = EchoRequest::parse(header.payload()) else {
        return Ok(false);
    };
    if request.len() > MAX_IPV4_PAYLOAD_LEN {
        return Ok(false);
    }

    let mut payload = [0u8; MAX_IPV4_PAYLOAD_LEN];
    request.write_reply(&mut payload[..request.len()])?;
    send_packet_to_mac(
        interface,
        dst_mac,
        header.source(),
        Ipv4Protocol::Icmp,
        &payload[..request.len()],
    )?;
    Ok(true)
}

pub fn send_packet(
    interface: &ManagedInterface,
    destination: Ipv4Addr,
    protocol: Ipv4Protocol,
    payload: &[u8],
) -> EResult<()> {
    let next_hop = interface.ipv4_next_hop(destination);
    let dst_mac = match interface.resolve_ipv4(destination) {
        Ok(mac) => mac,
        Err(Errno::EHOSTUNREACH) => {
            queue_pending_arp(interface, next_hop, destination, protocol, payload)?;
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    send_packet_to_mac(interface, dst_mac, destination, protocol, payload)
}

fn queue_pending_arp(
    interface: &ManagedInterface,
    next_hop: Ipv4Addr,
    destination: Ipv4Addr,
    protocol: Ipv4Protocol,
    payload: &[u8],
) -> EResult<()> {
    if payload.len() > MAX_IPV4_PAYLOAD_LEN {
        return Err(Errno::EMSGSIZE);
    }

    let mut pending = PENDING_ARP.lock();
    if pending.len() >= PENDING_ARP_LIMIT {
        return Err(Errno::ENOBUFS);
    }

    let mut packet = PendingArpPacket {
        interface: interface as *const ManagedInterface as usize,
        next_hop,
        destination,
        protocol,
        len: payload.len(),
        payload: [0; MAX_IPV4_PAYLOAD_LEN],
    };
    packet.payload[..payload.len()].copy_from_slice(payload);
    pending.push(packet);
    Ok(())
}

pub fn flush_pending_arp(interface: &ManagedInterface, next_hop: Ipv4Addr, dst_mac: MacAddr) {
    let interface_id = interface as *const ManagedInterface as usize;
    let mut ready = Vec::new();
    {
        let mut pending = PENDING_ARP.lock();
        let mut i = 0;
        while i < pending.len() {
            if pending[i].interface == interface_id && pending[i].next_hop == next_hop {
                ready.push(pending.remove(i));
            } else {
                i += 1;
            }
        }
    }

    for packet in ready {
        let _ = send_packet_to_mac(
            interface,
            dst_mac,
            packet.destination,
            packet.protocol,
            &packet.payload[..packet.len],
        );
    }
}

fn send_packet_to_mac(
    interface: &ManagedInterface,
    dst_mac: MacAddr,
    destination: Ipv4Addr,
    protocol: Ipv4Protocol,
    payload: &[u8],
) -> EResult<()> {
    if payload.len() > MAX_IPV4_PAYLOAD_LEN {
        return Err(Errno::EMSGSIZE);
    }

    let mut frame_payload = [0u8; MAX_ETH_PAYLOAD_LEN];
    let ipv4_payload = write_header(
        &mut frame_payload,
        interface.ip(),
        destination,
        protocol,
        payload.len(),
    )
    .ok_or(Errno::EINVAL)?;
    ipv4_payload.copy_from_slice(payload);
    interface.send_ethernet(
        dst_mac,
        EtherType::Ipv4,
        &frame_payload[..IPV4_HEADER_LEN + payload.len()],
    )
}

pub fn write_header(
    packet: &mut [u8],
    source: Ipv4Addr,
    destination: Ipv4Addr,
    protocol: Ipv4Protocol,
    payload_len: usize,
) -> Option<&mut [u8]> {
    let total_len = IPV4_HEADER_LEN.checked_add(payload_len)?;
    if packet.len() < total_len || total_len > u16::MAX as usize {
        return None;
    }

    packet[0] = (4 << 4) | 5;
    packet[1] = 0;
    packet[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    packet[4..6].copy_from_slice(&0u16.to_be_bytes());
    packet[6..8].copy_from_slice(&0u16.to_be_bytes());
    packet[8] = 64;
    packet[9] = protocol.as_u8();
    packet[10..12].copy_from_slice(&0u16.to_be_bytes());
    packet[12..16].copy_from_slice(source.as_bytes());
    packet[16..20].copy_from_slice(destination.as_bytes());

    let sum = checksum(&packet[..IPV4_HEADER_LEN]);
    packet[10..12].copy_from_slice(&sum.to_be_bytes());

    Some(&mut packet[IPV4_HEADER_LEN..total_len])
}

pub fn checksum(buf: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = buf.chunks_exact(2);

    for chunk in &mut chunks {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }

    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }

    !(sum as u16)
}
