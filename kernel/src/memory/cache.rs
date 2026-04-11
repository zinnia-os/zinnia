use crate::{
    arch::virt::get_page_size,
    device::block::BlockDevice,
    memory::{
        PhysAddr,
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
    },
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};
use alloc::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    sync::Arc,
};
use core::{fmt::Debug, num::NonZeroUsize, slice};

pub trait MemoryObject: Sync + Send {
    /// Attempts to get the physical address of a page with a relative index into this object.
    /// Returns [`None`] if the page is out of bounds for this object.
    fn try_get_page(&self, page_index: usize) -> Option<PhysAddr>;
}

#[derive(Debug)]
pub struct PagedMemoryObject {
    pages: SpinMutex<BTreeMap<usize, PhysAddr>>,
    dirty: SpinMutex<BTreeSet<usize>>,
    source: Arc<dyn Pager>,
}

impl PagedMemoryObject {
    /// Creates a new object, without making allocations.
    pub fn new(source: Arc<dyn Pager>) -> Self {
        Self {
            pages: SpinMutex::new(BTreeMap::new()),
            dirty: SpinMutex::new(BTreeSet::new()),
            source,
        }
    }

    /// Creates a new object with the physical memory allocator as a pager.
    pub fn new_phys() -> Self {
        Self::new(Arc::new(PhysPager))
    }

    /// Marks a page as dirty.
    pub fn mark_dirty(&self, page_index: usize) {
        self.dirty.lock().insert(page_index);
    }

    /// Writes all dirty pages back through the pager and clears the dirty set.
    pub fn sync(&self) -> EResult<()> {
        let pages = self.pages.lock();
        let mut dirty = self.dirty.lock();
        for &idx in dirty.iter() {
            if let Some(&addr) = pages.get(&idx) {
                self.source
                    .try_put_page(addr, idx)
                    .map_err(|_| Errno::EIO)?;
            }
        }
        dirty.clear();
        Ok(())
    }

    /// If a private mapping is requested, creates a new memory object and copies the data over.
    pub fn make_private(
        self: &Arc<Self>,
        length: NonZeroUsize,
        offset: usize,
    ) -> EResult<Arc<dyn MemoryObject>> {
        // Private mapping means we need to do a unique allocation.
        let phys = Arc::new(PagedMemoryObject::new_phys());
        (phys.as_ref() as &dyn MemoryObject).copy(
            offset as _,
            self.as_ref() as &dyn MemoryObject,
            offset as _,
            length.get(),
        );

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

    /// Copies from another memory object directly into [`self`].
    pub fn copy(
        &self,
        self_offset: usize,
        src: &dyn MemoryObject,
        src_offset: usize,
        len: usize,
    ) -> usize {
        let page_size = get_page_size();
        let mut progress = 0;

        while progress < len {
            let target_misalign = (progress + self_offset) % page_size;
            let src_misalign = (progress + src_offset) % page_size;

            let target_page_index = (progress + self_offset) / page_size;
            let src_page_index = (progress + src_offset) / page_size;

            let copy_size = (page_size - target_misalign)
                .min(page_size - src_misalign)
                .min(len - progress);

            let target_page = match self.try_get_page(target_page_index) {
                Some(x) => x,
                None => break,
            };

            let src_page = match src.try_get_page(src_page_index) {
                Some(x) => x,
                None => break,
            };

            let target_slice: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(target_page.as_hhdm(), page_size) };

            let src_slice: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(src_page.as_hhdm(), page_size) };

            target_slice[target_misalign..][..copy_size]
                .copy_from_slice(&src_slice[src_misalign..][..copy_size]);

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
    /// An I/O error occurred while reading/writing the page.
    IoError,
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

/// A pager backed by a block device.
/// Pages are read from / written to the device at a given byte offset.
pub struct BlockPager {
    device: Arc<dyn BlockDevice>,
    /// Byte offset into the device where this pager's data starts.
    byte_offset: u64,
}

impl Debug for BlockPager {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockPager")
            .field("byte_offset", &self.byte_offset)
            .finish()
    }
}

impl BlockPager {
    pub fn new(device: Arc<dyn BlockDevice>, byte_offset: u64) -> Self {
        Self {
            device,
            byte_offset,
        }
    }
}

impl Pager for BlockPager {
    fn has_page(&self, _page_index: usize) -> bool {
        true
    }

    fn try_get_page(&self, page_index: usize) -> Result<PhysAddr, PagerError> {
        let page_size = get_page_size();
        let phys =
            KernelAlloc::alloc(1, AllocFlags::empty()).map_err(|_| PagerError::OutOfMemory)?;

        let lba_size = self.device.get_lba_size();
        if lba_size == 0 {
            return Err(PagerError::IoError);
        }

        let offset = self.byte_offset + (page_index * page_size) as u64;
        let start_lba = offset / lba_size as u64;
        let num_lbas = page_size.div_ceil(lba_size);

        if self.device.read_lba(phys, num_lbas, start_lba).is_err() {
            unsafe { KernelAlloc::dealloc(phys, 1) };
            return Err(PagerError::IoError);
        }

        Ok(phys)
    }

    fn try_put_page(&self, address: PhysAddr, page_index: usize) -> Result<(), PagerError> {
        let page_size = get_page_size();
        let lba_size = self.device.get_lba_size();
        if lba_size == 0 {
            return Err(PagerError::IoError);
        }

        let offset = self.byte_offset + (page_index * page_size) as u64;
        let num_lbas = page_size / lba_size;

        for i in 0..num_lbas {
            let lba = offset / lba_size as u64 + i as u64;
            let buf = PhysAddr::new(address.value() + i * lba_size);
            self.device
                .write_lba(buf, lba)
                .map_err(|_| PagerError::IoError)?;
        }

        Ok(())
    }
}
