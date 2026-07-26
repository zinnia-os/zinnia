use crate::{
    arch::virt::get_page_size,
    device::{
        self,
        block::{BlockBuffer, BlockDevice, BlockOp, BlockSegment},
    },
    memory::{
        PhysAddr,
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
        virt::VmCacheType,
    },
    posix::errno::{EResult, Errno},
    util::mutex::spin::SpinMutex,
};
use alloc::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    sync::Arc,
    vec::Vec,
};
use core::{fmt::Debug, num::NonZeroUsize, slice};

const RA_MIN_PAGES: usize = 8;
const RA_MAX_PAGES: usize = 64;

pub trait MemoryObject: Sync + Send {
    /// Attempts to get the physical address of a page with a relative index into this object.
    fn try_get_page(&self, page_index: usize) -> EResult<Option<PhysAddr>>;

    fn mark_dirty_page(&self, page_index: usize) {
        let _ = page_index;
    }

    fn sync(&self) -> EResult<()> {
        Ok(())
    }

    /// The caching mode userspace mappings of this object should use.
    fn cache_type(&self) -> VmCacheType {
        VmCacheType::Normal
    }
}

#[derive(Debug)]
struct ReadAhead {
    next_expected: usize,
    window: usize,
}

#[derive(Debug)]
pub struct PagedMemoryObject {
    pages: SpinMutex<BTreeMap<usize, PhysAddr>>,
    dirty: SpinMutex<BTreeSet<usize>>,
    ra: SpinMutex<ReadAhead>,
    source: Arc<dyn Pager>,
}

impl PagedMemoryObject {
    /// Creates a new object, without making allocations.
    pub fn new(source: Arc<dyn Pager>) -> Self {
        Self {
            pages: SpinMutex::new(BTreeMap::new()),
            dirty: SpinMutex::new(BTreeSet::new()),
            ra: SpinMutex::new(ReadAhead {
                next_expected: usize::MAX,
                window: RA_MIN_PAGES,
            }),
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
        let dirty_pages = core::mem::take(&mut *self.dirty.lock())
            .into_iter()
            .collect::<Vec<_>>();

        self.writeback(dirty_pages)
    }

    pub fn sync_range(&self, start_page: usize, end_page: usize) -> EResult<()> {
        let dirty = {
            let mut set = self.dirty.lock();
            let in_range = set.range(start_page..end_page).copied().collect::<Vec<_>>();
            for idx in &in_range {
                set.remove(idx);
            }
            in_range
        };

        self.writeback(dirty)
    }

    fn writeback(&self, pages: Vec<usize>) -> EResult<()> {
        let mut result = Ok(());

        for idx in pages {
            let addr = self.pages.lock().get(&idx).copied();

            let Some(addr) = addr else {
                continue;
            };

            if self.source.try_put_page(addr, idx).is_err() {
                self.dirty.lock().insert(idx);
                result = Err(Errno::EIO);
            }
        }

        result
    }

    pub fn read_direct(&self, buf: &mut [u8], offset: usize) -> EResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let page_size = get_page_size();
        let start_page = offset / page_size;
        let end_page = (offset + buf.len()).div_ceil(page_size);
        self.sync_range(start_page, end_page)?;

        let mut progress = 0;
        while progress < buf.len() {
            let cur = offset + progress;
            let page_index = cur / page_size;
            let misalign = cur % page_size;
            let remaining = buf.len() - progress;
            let want = (misalign + remaining).div_ceil(page_size).min(RA_MAX_PAGES);

            let frames = self.source.try_get_pages(page_index, want)?;
            if frames.is_empty() {
                break;
            }

            for (i, frame) in frames.iter().enumerate() {
                if progress >= buf.len() {
                    break;
                }
                let page_off = if i == 0 { misalign } else { 0 };
                let copy = (page_size - page_off).min(buf.len() - progress);
                let src: &[u8] = unsafe { slice::from_raw_parts(frame.as_hhdm(), page_size) };
                buf[progress..][..copy].copy_from_slice(&src[page_off..][..copy]);
                progress += copy;
            }

            for frame in &frames {
                unsafe { KernelAlloc::dealloc(*frame, 1) };
            }
        }

        Ok(progress)
    }

    pub fn truncate(&self, length: usize) {
        let page_size = get_page_size();
        let page_count = length.div_ceil(page_size);
        let tail_offset = length % page_size;

        let mut pages = self.pages.lock();
        let tail_page = if tail_offset == 0 || page_count == 0 {
            None
        } else {
            pages.get(&(page_count - 1)).copied()
        };
        let removed_pages = pages.split_off(&page_count);
        drop(pages);

        if let Some(addr) = tail_page {
            let page = unsafe { slice::from_raw_parts_mut(addr.as_hhdm(), page_size) };
            page[tail_offset..].fill(0);
        }

        let mut dirty = self.dirty.lock();
        dirty.retain(|&page| page < page_count);
        if tail_page.is_some() {
            dirty.insert(page_count - 1);
        }
        drop(dirty);

        for (_, addr) in removed_pages {
            unsafe { KernelAlloc::dealloc(addr, 1) };
        }
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
        )?;

        Ok(phys)
    }
}

impl dyn MemoryObject {
    /// Reads data from the object into a buffer.
    pub fn read(&self, buffer: &mut [u8], offset: usize) -> EResult<usize> {
        let page_size = get_page_size();
        let mut progress = 0;

        while progress < buffer.len() {
            let misalign = (progress + offset) % page_size;
            let page_index = (progress + offset) / page_size;
            let copy_size = (page_size - misalign).min(buffer.len() - progress);

            let Some(page_addr) = self.try_get_page(page_index)? else {
                break;
            };

            let page_slice: &[u8] =
                unsafe { slice::from_raw_parts(page_addr.as_hhdm(), page_size) };
            buffer[progress..][..copy_size].copy_from_slice(&page_slice[misalign..][..copy_size]);
            progress += copy_size;
        }

        Ok(progress)
    }

    /// Writes data from a buffer into the object.
    pub fn write(&self, buffer: &[u8], offset: usize) -> EResult<usize> {
        let page_size = get_page_size();
        let mut progress = 0;

        while progress < buffer.len() {
            let misalign = (progress + offset) % page_size;
            let page_index = (progress + offset) / page_size;
            let copy_size = (page_size - misalign).min(buffer.len() - progress);

            let Some(page_addr) = self.try_get_page(page_index)? else {
                break;
            };

            let page_slice: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(page_addr.as_hhdm(), page_size) };
            page_slice[misalign..][..copy_size].copy_from_slice(&buffer[progress..][..copy_size]);
            progress += copy_size;
        }

        Ok(progress)
    }

    /// Copies from another memory object directly into [`self`].
    pub fn copy(
        &self,
        self_offset: usize,
        src: &dyn MemoryObject,
        src_offset: usize,
        len: usize,
    ) -> EResult<usize> {
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

            let Some(target_page) = self.try_get_page(target_page_index)? else {
                break;
            };

            let Some(src_page) = src.try_get_page(src_page_index)? else {
                break;
            };

            let target_slice: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(target_page.as_hhdm(), page_size) };

            let src_slice: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(src_page.as_hhdm(), page_size) };

            target_slice[target_misalign..][..copy_size]
                .copy_from_slice(&src_slice[src_misalign..][..copy_size]);

            progress += copy_size;
        }

        Ok(progress)
    }
}

impl MemoryObject for PagedMemoryObject {
    fn try_get_page(&self, page_index: usize) -> EResult<Option<PhysAddr>> {
        if let Some(page) = self.pages.lock().get(&page_index).copied() {
            return Ok(Some(page));
        }

        let mut count = 1;
        if self.source.readahead() {
            let mut ra = self.ra.lock();
            ra.window = if page_index == ra.next_expected {
                (ra.window * 2).min(RA_MAX_PAGES)
            } else {
                RA_MIN_PAGES
            };
            count = ra.window;
            ra.next_expected = page_index + count;
        }

        if count > 1 {
            let pages = self.pages.lock();
            for i in 1..count {
                if pages.contains_key(&(page_index + i)) {
                    count = i;
                    break;
                }
            }
        }

        let batch = match self.source.try_get_pages(page_index, count) {
            Ok(batch) if !batch.is_empty() => batch,
            Ok(_) | Err(PagerError::IndexOutOfBounds) => return Ok(None),
            Err(PagerError::OutOfMemory) if count > 1 => {
                match self.source.try_get_pages(page_index, 1) {
                    Ok(batch) if !batch.is_empty() => batch,
                    Ok(_) => return Ok(None),
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        };

        let mut pages = self.pages.lock();
        let mut result = None;
        for (i, page) in batch.into_iter().enumerate() {
            let idx = page_index + i;
            match pages.get(&idx).copied() {
                Some(existing) => {
                    unsafe { KernelAlloc::dealloc(page, 1) };
                    if i == 0 {
                        result = Some(existing);
                    }
                }
                None => {
                    pages.insert(idx, page);
                    if i == 0 {
                        result = Some(page);
                    }
                }
            }
        }
        Ok(result)
    }

    fn mark_dirty_page(&self, page_index: usize) {
        self.mark_dirty(page_index);
    }

    fn sync(&self) -> EResult<()> {
        PagedMemoryObject::sync(self)
    }
}

impl Drop for PagedMemoryObject {
    fn drop(&mut self) {
        if let Err(e) = self.sync() {
            warn!("Dropping page cache object with unflushed dirty pages: {e:?}");
        }

        let p = self.pages.lock();
        for (_, &addr) in p.iter() {
            unsafe { KernelAlloc::dealloc(addr, 1) };
        }
    }
}

/// Used to get new data for a memory object.
pub trait Pager: Sync + Send + Debug {
    /// Checks to see if the pager has data at the given offset.
    fn has_page(&self, page_index: usize) -> bool;
    /// Attempts to get a page at an index.
    fn try_get_page(&self, page_index: usize) -> Result<PhysAddr, PagerError>;
    /// Attempts to write a page at an index back to the device.
    fn try_put_page(&self, address: PhysAddr, page_index: usize) -> Result<(), PagerError>;

    fn readahead(&self) -> bool {
        false
    }

    fn try_get_pages(&self, page_index: usize, count: usize) -> Result<Vec<PhysAddr>, PagerError> {
        let mut pages = Vec::with_capacity(count);
        for i in 0..count {
            match self.try_get_page(page_index + i) {
                Ok(page) => pages.push(page),
                Err(PagerError::IndexOutOfBounds) => break,
                Err(e) => {
                    for page in pages {
                        unsafe { KernelAlloc::dealloc(page, 1) };
                    }
                    return Err(e);
                }
            }
        }
        Ok(pages)
    }
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

impl From<PagerError> for Errno {
    fn from(value: PagerError) -> Self {
        match value {
            PagerError::IndexOutOfBounds => Errno::EINVAL,
            PagerError::OutOfMemory => Errno::ENOMEM,
            PagerError::IoError => Errno::EIO,
        }
    }
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

    fn readahead(&self) -> bool {
        true
    }

    fn try_get_pages(&self, page_index: usize, count: usize) -> Result<Vec<PhysAddr>, PagerError> {
        let page_size = get_page_size();
        let lba_size = self.device.get_lba_size();
        if lba_size == 0 || !page_size.is_multiple_of(lba_size) {
            return Err(PagerError::IoError);
        }
        let lbas_per_page = page_size / lba_size;

        let mut pages = Vec::with_capacity(count);
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            match KernelAlloc::alloc(1, AllocFlags::empty()) {
                Ok(page) => {
                    segments.push(BlockSegment::new(page, page_size));
                    pages.push(page);
                }
                Err(_) => {
                    for page in pages {
                        unsafe { KernelAlloc::dealloc(page, 1) };
                    }
                    return Err(PagerError::OutOfMemory);
                }
            }
        }

        let offset = self.byte_offset + (page_index * page_size) as u64;
        let start_lba = offset / lba_size as u64;
        let read = match device::block::submit_all(
            self.device.as_ref(),
            BlockOp::Read,
            start_lba,
            count * lbas_per_page,
            &segments,
        ) {
            Ok(read) => read,
            Err(_) => {
                for page in pages {
                    unsafe { KernelAlloc::dealloc(page, 1) };
                }
                return Err(PagerError::IoError);
            }
        };

        let full_pages = read / lbas_per_page;
        for page in pages.drain(full_pages..) {
            unsafe { KernelAlloc::dealloc(page, 1) };
        }
        Ok(pages)
    }

    fn try_get_page(&self, page_index: usize) -> Result<PhysAddr, PagerError> {
        let page_size = get_page_size();
        let mut buffer = BlockBuffer::new(page_size).map_err(|_| PagerError::OutOfMemory)?;

        let lba_size = self.device.get_lba_size();
        if lba_size == 0 || !page_size.is_multiple_of(lba_size) {
            return Err(PagerError::IoError);
        }

        let offset = self.byte_offset + (page_index * page_size) as u64;
        let start_lba = offset / lba_size as u64;
        let num_lbas = page_size / lba_size;

        if device::block::read_exact_into(self.device.as_ref(), &mut buffer, num_lbas, start_lba)
            .is_err()
        {
            return Err(PagerError::IoError);
        }

        let (phys, _) = buffer.into_phys();
        Ok(phys)
    }

    fn try_put_page(&self, address: PhysAddr, page_index: usize) -> Result<(), PagerError> {
        let page_size = get_page_size();
        let lba_size = self.device.get_lba_size();
        if lba_size == 0 || !page_size.is_multiple_of(lba_size) {
            return Err(PagerError::IoError);
        }

        let offset = self.byte_offset + (page_index * page_size) as u64;
        let start_lba = offset / lba_size as u64;
        let num_lbas = page_size / lba_size;
        let seg = [BlockSegment::new(address, page_size)];

        let written = device::block::submit_all(
            self.device.as_ref(),
            BlockOp::Write,
            start_lba,
            num_lbas,
            &seg,
        )
        .map_err(|_| PagerError::IoError)?;
        if written < num_lbas {
            return Err(PagerError::IoError);
        }

        Ok(())
    }
}
