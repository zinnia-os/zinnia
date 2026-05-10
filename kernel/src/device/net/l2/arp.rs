//! Address Resolution Protocol (RFC 826) for IPv4-over-Ethernet.

use crate::{
    device::net::{
        interface::ManagedInterface,
        l2::{
            eth::{EthHeader, EtherType},
            mac::MacAddr,
        },
        l3::ipv4::{self, Ipv4Addr},
    },
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};
use alloc::collections::btree_map::BTreeMap;
use num_enum::TryFromPrimitive;

const HTYPE_ETHERNET: u16 = 1;
const PTYPE_IPV4: u16 = EtherType::Ipv4 as u16;

const HLEN_ETHERNET: u8 = 6;
const PLEN_IPV4: u8 = 4;

pub const ARP_PACKET_LEN: usize = 28;

#[derive(Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u16)]
pub enum ArpOp {
    Request = 1,
    Reply = 2,
}

#[derive(Clone, Copy)]
pub struct ArpPacket {
    pub op: ArpOp,
    pub sender_mac: MacAddr,
    pub sender_ip: Ipv4Addr,
    pub target_mac: MacAddr,
    pub target_ip: Ipv4Addr,
}

impl ArpPacket {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < ARP_PACKET_LEN {
            return None;
        }

        let htype = u16::from_be_bytes([buf[0], buf[1]]);
        let ptype = u16::from_be_bytes([buf[2], buf[3]]);
        let hlen = buf[4];
        let plen = buf[5];
        let oper = u16::from_be_bytes([buf[6], buf[7]]);

        if htype != HTYPE_ETHERNET
            || ptype != PTYPE_IPV4
            || hlen != HLEN_ETHERNET
            || plen != PLEN_IPV4
        {
            return None;
        }

        let op = ArpOp::try_from(oper).ok()?;

        let mut sha = [0u8; 6];
        sha.copy_from_slice(&buf[8..14]);
        let mut spa = [0u8; 4];
        spa.copy_from_slice(&buf[14..18]);
        let mut tha = [0u8; 6];
        tha.copy_from_slice(&buf[18..24]);
        let mut tpa = [0u8; 4];
        tpa.copy_from_slice(&buf[24..28]);

        Some(Self {
            op,
            sender_mac: MacAddr::new(&sha),
            sender_ip: Ipv4Addr::new(spa),
            target_mac: MacAddr::new(&tha),
            target_ip: Ipv4Addr::new(tpa),
        })
    }

    pub fn write(&self, buf: &mut [u8]) -> EResult<()> {
        if buf.len() < ARP_PACKET_LEN {
            return Err(Errno::EINVAL);
        }

        buf[0..2].copy_from_slice(&HTYPE_ETHERNET.to_be_bytes());
        buf[2..4].copy_from_slice(&PTYPE_IPV4.to_be_bytes());
        buf[4] = HLEN_ETHERNET;
        buf[5] = PLEN_IPV4;
        buf[6..8].copy_from_slice(&(self.op as u16).to_be_bytes());
        buf[8..14].copy_from_slice(self.sender_mac.as_bytes());
        buf[14..18].copy_from_slice(self.sender_ip.as_bytes());
        buf[18..24].copy_from_slice(self.target_mac.as_bytes());
        buf[24..28].copy_from_slice(self.target_ip.as_bytes());
        Ok(())
    }
}

pub struct ArpCache {
    entries: SpinMutex<BTreeMap<Ipv4Addr, MacAddr>>,
}

impl ArpCache {
    pub const fn new() -> Self {
        Self {
            entries: SpinMutex::new(BTreeMap::new()),
        }
    }

    pub fn insert(&self, ip: Ipv4Addr, mac: MacAddr) {
        self.entries.lock().insert(ip, mac);
    }

    pub fn lookup(&self, ip: &Ipv4Addr) -> Option<MacAddr> {
        self.entries.lock().get(ip).copied()
    }
}

pub fn process_packet(interface: &ManagedInterface, eth: &EthHeader<'_>) -> EResult<bool> {
    let Some(packet) = ArpPacket::parse(eth.payload()) else {
        return Ok(false);
    };

    if packet.sender_ip != Ipv4Addr::ANY {
        interface
            .arp_cache()
            .insert(packet.sender_ip, packet.sender_mac);
        ipv4::flush_pending_arp(interface, packet.sender_ip, packet.sender_mac);
    }

    if packet.op == ArpOp::Request && packet.target_ip == interface.ip() {
        send_reply(interface, &packet)?;
    }

    Ok(true)
}

pub fn send_request(interface: &ManagedInterface, target_ip: Ipv4Addr) -> EResult<()> {
    let packet = ArpPacket {
        op: ArpOp::Request,
        sender_mac: interface.mac(),
        sender_ip: interface.ip(),
        target_mac: MacAddr::ZERO,
        target_ip,
    };
    send_packet(interface, MacAddr::BROADCAST, &packet)
}

fn send_reply(interface: &ManagedInterface, request: &ArpPacket) -> EResult<()> {
    let packet = ArpPacket {
        op: ArpOp::Reply,
        sender_mac: interface.mac(),
        sender_ip: interface.ip(),
        target_mac: request.sender_mac,
        target_ip: request.sender_ip,
    };
    log!(
        "Replying to {} ({}): {} is at {}",
        request.sender_ip,
        request.sender_mac,
        interface.ip(),
        interface.mac()
    );
    send_packet(interface, request.sender_mac, &packet)
}

fn send_packet(interface: &ManagedInterface, dst: MacAddr, packet: &ArpPacket) -> EResult<()> {
    let mut payload = [0u8; ARP_PACKET_LEN];
    packet.write(&mut payload)?;
    interface.send_ethernet(dst, EtherType::Arp, &payload)
}
