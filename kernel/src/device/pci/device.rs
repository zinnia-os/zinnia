use super::config::Address;
use crate::{posix::errno::EResult, util::mutex::spin::SpinMutex};
use alloc::vec::Vec;

pub trait Device {
    /// Returns the PCI address of this device.
    fn address(&self) -> Address;

    /// Called when a device is put to sleep.
    fn suspend(&self) -> EResult<()> {
        Ok(())
    }

    /// Called when a device is woken up.
    fn resume(&self) -> EResult<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    Mmio32 {
        address: u32,
        size: usize,
        prefetchable: bool,
    },
    Mmio64 {
        address: u64,
        size: usize,
        prefetchable: bool,
    },
    Io {
        address: u16,
        size: usize,
    },
}

impl PciBar {
    pub fn is_valid(&self) -> bool {
        match self {
            PciBar::Mmio32 { address, .. } => *address != 0,
            PciBar::Mmio64 { address, .. } => *address != 0,
            PciBar::Io { address, .. } => *address != 0,
        }
    }
}

pub static PCI_DEVICES: SpinMutex<Vec<Address>> = SpinMutex::new(Vec::new());
