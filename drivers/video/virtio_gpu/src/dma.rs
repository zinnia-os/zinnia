use crate::error::GpuError;
use zinnia::{
    arch,
    memory::{AllocFlags, OwnedPhysPages, PhysAddr},
};

pub struct DmaRegion {
    pages: OwnedPhysPages,
    len: usize,
}

impl DmaRegion {
    pub fn new(len: usize) -> Result<Self, GpuError> {
        let page_size = arch::virt::get_page_size();
        let page_count = len.div_ceil(page_size).max(1);
        let pages = OwnedPhysPages::new(page_count, AllocFlags::empty())
            .map_err(|_| GpuError::AllocationFailed)?;
        Ok(Self { pages, len })
    }

    pub fn phys(&self) -> PhysAddr {
        self.pages.phys()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn page_at(&self, index: usize) -> Option<PhysAddr> {
        let page_size = arch::virt::get_page_size();
        let offset = index.checked_mul(page_size)?;
        (offset < self.len).then(|| self.pages.phys() + offset)
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.pages.as_hhdm::<u8>(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.pages.as_hhdm::<u8>(), self.len) }
    }
}
