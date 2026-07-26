use crate::{
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr},
    posix::errno::{EResult, Errno},
};
use core::{mem::ManuallyDrop, slice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOp {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSegment {
    phys: PhysAddr,
    len: usize,
}

impl BlockSegment {
    pub const fn new(phys: PhysAddr, len: usize) -> Self {
        Self { phys, len }
    }

    pub const fn phys(&self) -> PhysAddr {
        self.phys
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

pub struct BlockBuffer {
    phys: PhysAddr,
    len: usize,
}

impl BlockBuffer {
    pub fn new(len: usize) -> EResult<Self> {
        if len == 0 {
            return Err(Errno::EINVAL);
        }

        let phys = KernelAlloc::alloc_bytes(len, AllocFlags::empty()).map_err(|_| Errno::ENOMEM)?;
        Ok(Self { phys, len })
    }

    pub const fn phys(&self) -> PhysAddr {
        self.phys
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.phys.as_hhdm(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.phys.as_hhdm(), self.len) }
    }

    pub fn into_phys(self) -> (PhysAddr, usize) {
        let this = ManuallyDrop::new(self);
        (this.phys, this.len)
    }
}

impl Drop for BlockBuffer {
    fn drop(&mut self) {
        unsafe { KernelAlloc::dealloc_bytes(self.phys, self.len) };
    }
}
