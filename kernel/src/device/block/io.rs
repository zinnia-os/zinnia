use crate::{
    memory::{AllocFlags, KernelAlloc, PageAllocator, PhysAddr},
    posix::errno::{EResult, Errno},
};
use core::{marker::PhantomData, mem::ManuallyDrop, slice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOp {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockCompletion {
    pub lbas: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSegment {
    phys: PhysAddr,
    len: usize,
}

impl BlockSegment {
    pub const fn phys(&self) -> PhysAddr {
        self.phys
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) const fn from_phys(phys: PhysAddr, len: usize) -> Self {
        Self { phys, len }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockIter {
    lba: u64,
    bytes: usize,
    segment_idx: usize,
    segment_done: usize,
}

impl BlockIter {
    pub const fn lba(&self) -> u64 {
        self.lba
    }

    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub const fn segment_idx(&self) -> usize {
        self.segment_idx
    }

    pub const fn segment_done(&self) -> usize {
        self.segment_done
    }
}

pub struct BlockIo<'a> {
    op: BlockOp,
    lba: u64,
    num_lbas: usize,
    bytes: usize,
    segments: [BlockSegment; 1],
    iter: BlockIter,
    _lifetime: PhantomData<&'a ()>,
}

impl<'a> BlockIo<'a> {
    pub fn read(
        buffer: &'a mut BlockBuffer,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        Self::read_at(buffer, 0, lba, num_lbas, lba_size)
    }

    pub fn read_at(
        buffer: &'a mut BlockBuffer,
        offset: usize,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        let len = buffer.len.checked_sub(offset).ok_or(Errno::EINVAL)?;
        Self::from_buffer(
            BlockOp::Read,
            buffer.phys + offset,
            len,
            lba,
            num_lbas,
            lba_size,
        )
    }

    pub fn write(
        buffer: &'a BlockBuffer,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        Self::write_at(buffer, 0, lba, num_lbas, lba_size)
    }

    pub fn write_at(
        buffer: &'a BlockBuffer,
        offset: usize,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        let len = buffer.len.checked_sub(offset).ok_or(Errno::EINVAL)?;
        Self::from_buffer(
            BlockOp::Write,
            buffer.phys + offset,
            len,
            lba,
            num_lbas,
            lba_size,
        )
    }

    pub(crate) fn read_phys(
        phys: PhysAddr,
        len: usize,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        Self::from_buffer(BlockOp::Read, phys, len, lba, num_lbas, lba_size)
    }

    pub(crate) fn write_phys(
        phys: PhysAddr,
        len: usize,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        Self::from_buffer(BlockOp::Write, phys, len, lba, num_lbas, lba_size)
    }

    fn from_buffer(
        op: BlockOp,
        phys: PhysAddr,
        len: usize,
        lba: u64,
        num_lbas: usize,
        lba_size: usize,
    ) -> EResult<Self> {
        if lba_size == 0 || num_lbas == 0 {
            return Err(Errno::EINVAL);
        }

        let bytes = num_lbas.checked_mul(lba_size).ok_or(Errno::EOVERFLOW)?;
        if bytes > len {
            return Err(Errno::EINVAL);
        }

        Ok(Self {
            op,
            lba,
            num_lbas,
            bytes,
            segments: [BlockSegment::from_phys(phys, bytes)],
            iter: BlockIter {
                lba,
                bytes,
                segment_idx: 0,
                segment_done: 0,
            },
            _lifetime: PhantomData,
        })
    }

    pub const fn op(&self) -> BlockOp {
        self.op
    }

    pub const fn lba(&self) -> u64 {
        self.lba
    }

    pub fn set_lba(&mut self, lba: u64) {
        self.lba = lba;
        self.iter.lba = lba;
    }

    pub const fn num_lbas(&self) -> usize {
        self.num_lbas
    }

    pub fn set_num_lbas(&mut self, num_lbas: usize, lba_size: usize) -> EResult<()> {
        if lba_size == 0 || num_lbas == 0 {
            return Err(Errno::EINVAL);
        }

        let bytes = num_lbas.checked_mul(lba_size).ok_or(Errno::EOVERFLOW)?;
        let segment_len = self.segments[0].len;
        if bytes > segment_len {
            return Err(Errno::EINVAL);
        }

        self.num_lbas = num_lbas;
        self.bytes = bytes;
        self.segments[0].len = bytes;
        self.iter.bytes = bytes;
        Ok(())
    }

    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub const fn iter(&self) -> BlockIter {
        self.iter
    }

    pub fn segments(&self) -> &[BlockSegment] {
        &self.segments
    }

    pub const fn first_segment(&self) -> BlockSegment {
        self.segments[0]
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
