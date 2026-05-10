use crate::posix::errno::{EResult, Errno};

pub const ICMP_HEADER_LEN: usize = 8;

const ICMP_ECHO_REPLY: u8 = 0;
const ICMP_ECHO_REQUEST: u8 = 8;

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
        if super::ipv4::checksum(packet) != 0 {
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

        let sum = super::ipv4::checksum(&packet[..self.body.len()]);
        packet[2..4].copy_from_slice(&sum.to_be_bytes());
        Ok(())
    }
}
