use crate::memory::VirtAddr;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct iovec {
    pub base: VirtAddr,
    pub len: usize,
}
