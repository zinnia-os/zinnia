use crate::spec;
use zinnia::{
    arch,
    memory::{AllocFlags, OwnedPhysPages, PhysAddr},
    posix::errno::EResult,
};

pub struct Ring {
    pages: OwnedPhysPages,
    /// Ring size in TRBs.
    pub size: usize,
    /// Index of the next TRB to produce/consume.
    pub index: usize,
    /// Producer/consumer cycle state.
    pub cycle: bool,
}

impl Ring {
    pub fn new() -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        let pages = OwnedPhysPages::new(1, AllocFlags::empty())?;
        unsafe { core::ptr::write_bytes(pages.as_hhdm::<u8>(), 0, page_size) };

        Ok(Self {
            size: page_size / spec::TRB_SIZE,
            pages,
            index: 0,
            cycle: true,
        })
    }

    pub fn phys(&self) -> PhysAddr {
        self.pages.phys()
    }
}
