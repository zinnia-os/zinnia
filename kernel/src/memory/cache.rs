use crate::{
    arch::virt::get_page_size,
    memory::{
        PhysAddr,
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
    },
    posix::errno::EResult,
    uapi,
    util::mutex::spin::SpinMutex,
};
use alloc::{collections::btree_map::BTreeMap, sync::Arc};
use core::{fmt::Debug, num::NonZeroUsize, slice};

pub trait MemoryObject: Sync + Send {
    /// Attempts to get the physical address of a page with a relative index into this object.
    /// Returns [`None`] if the page is out of bounds for this object.
    fn try_get_page(&self, page_index: usize) -> Option<PhysAddr>;
}

#[derive(Debug)]
pub struct PagedMemoryObject {
    pages: SpinMutex<BTreeMap<usize, PhysAddr>>,
    source: Arc<dyn Pager>,
}

impl PagedMemoryObject {
    /// Creates a new object, without making allocations.
    pub fn new(source: Arc<dyn Pager>) -> Self {
        Self {
            pages: SpinMutex::new(BTreeMap::new()),
            source,
        }
    }

    /// Creates a new object with the physical memory allocator as a pager.
    pub fn new_phys() -> Self {
        Self::new(Arc::new(PhysPager))
    }

    /// If a private mapping is requested, creates a new memory object and copies the data over.
    pub fn make_private(
        self: &Arc<Self>,
        length: NonZeroUsize,
        offset: uapi::off_t,
    ) -> EResult<Arc<dyn MemoryObject>> {
        // Private mapping means we need to do a unique allocation.
        // TODO: Do this in smaller chunks to not overwhelm the allocator.
        let phys = Arc::new(PagedMemoryObject::new_phys());
        let mut buf = vec![0u8; length.into()];
        (self.as_ref() as &dyn MemoryObject).read(&mut buf, offset as _);
        (phys.as_ref() as &dyn MemoryObject).write(&buf, offset as _);
        Ok(phys)
    }
}

impl dyn MemoryObject {
    /// Reads data from the object into a buffer.
    /// Reading out of bounds will return 0.
    pub fn read(&self, buffer: &mut [u8], offset: usize) -> usize {
        let page_size = get_page_size();
        let mut progress = 0;

        while progress < buffer.len() {
            let misalign = (progress + offset) % page_size;
            let page_index = (progress + offset) / page_size;
            let copy_size = (page_size - misalign).min(buffer.len() - progress);

            let page_addr = match self.try_get_page(page_index) {
                Some(x) => x,
                None => break,
            };

            let page_slice: &[u8] =
                unsafe { slice::from_raw_parts(page_addr.as_hhdm(), page_size) };
            buffer[progress..][..copy_size].copy_from_slice(&page_slice[misalign..][..copy_size]);
            progress += copy_size;
        }

        progress
    }

    /// Writes data from a buffer into the object.
    /// Writing out of bounds will return 0.
    pub fn write(&self, buffer: &[u8], offset: usize) -> usize {
        let page_size = get_page_size();
        let mut progress = 0;

        while progress < buffer.len() {
            let misalign = (progress + offset) % page_size;
            let page_index = (progress + offset) / page_size;
            let copy_size = (page_size - misalign).min(buffer.len() - progress);

            let page_addr = match self.try_get_page(page_index) {
                Some(x) => x,
                None => break,
            };

            let page_slice: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(page_addr.as_hhdm(), page_size) };
            page_slice[misalign..][..copy_size].copy_from_slice(&buffer[progress..][..copy_size]);
            progress += copy_size;
        }

        progress
    }
}

impl MemoryObject for PagedMemoryObject {
    fn try_get_page(&self, page_index: usize) -> Option<PhysAddr> {
        let mut pages = self.pages.lock();
        match pages.get(&page_index) {
            // If the page already exists, we can return it.
            Some(page) => Some(*page),
            // If it does not, we need to check if it's actually available.
            None => match self.source.try_get_page(page_index) {
                Ok(x) => {
                    pages.insert(page_index, x);
                    Some(x)
                }
                Err(_) => None,
            },
        }
    }
}

impl Drop for PagedMemoryObject {
    fn drop(&mut self) {
        let p = self.pages.lock();
        for (_, &addr) in p.iter() {
            unsafe { KernelAlloc::dealloc(addr, 1) };
        }
    }
}

/// Used to get new data for a memory object.
// TODO: Vectorized IO.
pub trait Pager: Sync + Send + Debug {
    /// Checks to see if the pager has data at the given offset.
    fn has_page(&self, page_index: usize) -> bool;
    /// Attempts to get a page at an index.
    fn try_get_page(&self, page_index: usize) -> Result<PhysAddr, PagerError>;
    /// Attempts to write a page at an index back to the device.
    fn try_put_page(&self, address: PhysAddr, page_index: usize) -> Result<(), PagerError>;
}

/// Errors that can occur when reading or writing a page.
pub enum PagerError {
    /// The page at a given index is out of bounds.
    IndexOutOfBounds,
    /// The pager cannot allocate pages.
    OutOfMemory,
}

/// A pager which uses kernel memory to get physical pages.
#[derive(Debug)]
struct PhysPager;
impl Pager for PhysPager {
    fn has_page(&self, _: usize) -> bool {
        // We always have pages.
        // TODO: We don't if we're close to running out of memory.
        true
    }

    fn try_get_page(&self, _: usize) -> Result<PhysAddr, PagerError> {
        KernelAlloc::alloc(1, AllocFlags::empty()).map_err(|_| PagerError::OutOfMemory)
    }

    fn try_put_page(&self, _: PhysAddr, _: usize) -> Result<(), PagerError> {
        // Don't do anything. There's nothing to write back to.
        Ok(())
    }
}
