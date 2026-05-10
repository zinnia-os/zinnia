use crate::device::net::l2::mac::MacAddr;
use num_enum::TryFromPrimitive;

pub const ETH_HEADER_LEN: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u16)]
pub enum EtherType {
    Ipv4 = 0x0800,
    Arp = 0x0806,
    Ipv6 = 0x86dd,
}

/// Ethernet II header at the start of a frame.
pub struct EthHeader<'a> {
    frame: &'a [u8],
}

impl<'a> EthHeader<'a> {
    pub fn parse(frame: &'a [u8]) -> Option<Self> {
        if frame.len() < ETH_HEADER_LEN {
            return None;
        }
        Some(Self { frame })
    }

    pub fn dst(&self) -> MacAddr {
        let mut b = [0u8; 6];
        b.copy_from_slice(&self.frame[0..6]);
        MacAddr::new(&b)
    }

    pub fn src(&self) -> MacAddr {
        let mut b = [0u8; 6];
        b.copy_from_slice(&self.frame[6..12]);
        MacAddr::new(&b)
    }

    pub fn ethertype(&self) -> Option<EtherType> {
        EtherType::try_from(u16::from_be_bytes([self.frame[12], self.frame[13]])).ok()
    }

    pub fn payload(&self) -> &'a [u8] {
        &self.frame[ETH_HEADER_LEN..]
    }
}

/// Returns the slice covering the payload area, or [`None`] if the buffer is too small.
pub fn write_header<'a>(
    frame: &'a mut [u8],
    dst: &MacAddr,
    src: &MacAddr,
    ethertype: EtherType,
) -> Option<&'a mut [u8]> {
    if frame.len() < ETH_HEADER_LEN {
        return None;
    }
    frame[0..6].copy_from_slice(dst.as_bytes());
    frame[6..12].copy_from_slice(src.as_bytes());
    frame[12..14].copy_from_slice(&(ethertype as u16).to_be_bytes());
    Some(&mut frame[ETH_HEADER_LEN..])
}
