use super::config::Address;
use crate::util::mutex::spin::SpinMutex;
use alloc::vec::Vec;

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
