use crate::{device::net::l2::mac::MacAddr, posix::errno::EResult};

/// Represents a network interface controller.
pub trait NicDevice: Send + Sync {
    /// The hardware address of this NIC.
    fn mac(&self) -> MacAddr;

    /// Receives a frame from the device. Returns the number of bytes written into `frame`.
    fn recv(&self, frame: &mut [u8]) -> EResult<usize>;

    /// Sends a frame to the device.
    fn send(&self, frame: &[u8]) -> EResult<()>;
}
