use crate::{
    arch,
    memory::VirtAddr,
    posix::errno::{EResult, Errno},
    uapi::uio::iovec,
};

/// An [`IovecIter`] can *never* be used outside of the address space it was created in.
#[derive(Debug)]
pub struct IovecIter<'a> {
    iovecs: &'a [iovec],
    total_len: usize,
    total_offset: usize,
    current_idx: usize,
    current_offset: usize,
}

impl<'a> !Send for IovecIter<'a> {}
impl<'a> !Sync for IovecIter<'a> {}

impl<'a> IovecIter<'a> {
    pub fn new(iovecs: &'a [iovec]) -> EResult<Self> {
        // Check if all addresses are in user memory.
        for i in iovecs {
            if i.len != 0 && !arch::virt::is_user_addr(i.base + VirtAddr::new(i.len)) {
                return Err(Errno::EFAULT);
            }
        }

        let mut total_len: usize = 0;
        for i in iovecs.iter() {
            total_len = total_len.checked_add(i.len).ok_or(Errno::EINVAL)?;
        }

        Ok(Self {
            iovecs,
            total_len,
            total_offset: 0,
            current_idx: 0,
            current_offset: 0,
        })
    }

    /// Creates an iovec from kernel memory.
    /// # Safety
    /// Only valid inside a kernel context.
    pub unsafe fn iovec_from_ptr(bytes: &[u8]) -> iovec {
        iovec {
            base: VirtAddr::from(bytes.as_ptr()),
            len: bytes.len(),
        }
    }

    /// Creates an iovec from mutable kernel memory.
    /// # Safety
    /// Only valid inside a kernel context.
    pub unsafe fn iovec_from_mut_ptr(bytes: &mut [u8]) -> iovec {
        iovec {
            base: VirtAddr::from(bytes.as_mut_ptr()),
            len: bytes.len(),
        }
    }

    /// Creates a new iterator for kernel accesses.
    /// # Safety
    /// Only valid for kernel iovecs.
    pub unsafe fn new_kernel(iovecs: &'a [iovec]) -> Self {
        // Check if all addresses are in kernel memory.
        for i in iovecs {
            assert!(i.len == 0 || !arch::virt::is_user_addr(i.base + VirtAddr::new(i.len)));
        }

        Self {
            iovecs,
            total_len: iovecs.iter().map(|x| x.len).sum(),
            total_offset: 0,
            current_idx: 0,
            current_offset: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.total_len
    }

    pub const fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    pub const fn total_offset(&self) -> usize {
        self.total_offset
    }

    pub const fn is_finished(&self) -> bool {
        self.total_len == self.total_offset
    }

    pub fn skip(&mut self, mut count: usize) -> usize {
        loop {
            let remaining_current = self.iovecs[self.current_idx].len - self.current_offset;
            let skip_current = remaining_current.min(count);

            count -= skip_current;

            self.total_offset += skip_current;
            self.current_offset += skip_current;

            if self.current_offset == self.iovecs[self.current_idx].len && !self.is_finished() {
                self.current_idx += 1;
                self.current_offset = 0;
            }

            if count == 0 || self.is_finished() {
                break;
            }
        }

        self.total_len - self.total_offset
    }

    pub fn set_offset(&mut self, offset: usize) -> usize {
        self.current_idx = 0;
        self.current_offset = 0;
        self.total_offset = 0;

        self.skip(offset)
    }

    pub fn fill(&mut self, value: u8) -> EResult<()> {
        let old_offset = self.total_offset;
        let mut remaining_total = self.total_len - self.total_offset;

        loop {
            self.skip(0);
            let remaining_current = self.iovecs[self.current_idx].len - self.current_offset;
            let copy_current = remaining_current.min(remaining_total);

            let addr = self.iovecs[self.current_idx].base + self.current_offset;

            if arch::virt::is_user_addr(addr) {
                // TODO: Add User memset.
                for b in 0..copy_current {
                    if !arch::virt::copy_to_user(addr + b, &[value]) {
                        self.set_offset(old_offset);
                        return Err(Errno::EFAULT);
                    }
                }
            } else {
                let dest = unsafe { core::slice::from_raw_parts_mut(addr.as_ptr(), copy_current) };
                dest.fill(value);
            }

            self.skip(copy_current);
            remaining_total -= copy_current;

            if self.is_finished() || remaining_total == 0 {
                break;
            }
        }

        Ok(())
    }

    pub fn copy_from_slice(&mut self, slice: &[u8]) -> EResult<()> {
        let old_offset = self.total_offset;
        let mut remaining_total = slice.len().min(self.total_len - self.total_offset);
        let mut total_done = 0;

        loop {
            self.skip(0);
            let remaining_current = self.iovecs[self.current_idx].len - self.current_offset;
            let copy_current = remaining_current.min(remaining_total);

            let addr = self.iovecs[self.current_idx].base + self.current_offset;
            let src = &slice[total_done..][..copy_current];

            if arch::virt::is_user_addr(addr) {
                if !arch::virt::copy_to_user(addr, src) {
                    self.set_offset(old_offset);
                    return Err(Errno::EFAULT);
                }
            } else {
                let dest = unsafe { core::slice::from_raw_parts_mut(addr.as_ptr(), src.len()) };
                dest.copy_from_slice(src);
            }

            self.skip(copy_current);
            total_done += copy_current;
            remaining_total -= copy_current;

            if self.is_finished() || remaining_total == 0 {
                break;
            }
        }

        Ok(())
    }

    pub fn copy_to_slice(&mut self, slice: &mut [u8]) -> EResult<()> {
        let old_offset = self.total_offset;
        let mut remaining_total = slice.len().min(self.total_len - self.total_offset);
        let mut total_done = 0;

        loop {
            self.skip(0);
            let remaining_current = self.iovecs[self.current_idx].len - self.current_offset;
            let copy_current = remaining_current.min(remaining_total);

            let addr = self.iovecs[self.current_idx].base + self.current_offset;
            let dest = &mut slice[total_done..][..copy_current];

            if arch::virt::is_user_addr(addr) {
                if !arch::virt::copy_from_user(dest, addr) {
                    self.set_offset(old_offset);
                    return Err(Errno::EFAULT);
                }
            } else {
                let src = unsafe { core::slice::from_raw_parts(addr.as_ptr(), dest.len()) };
                dest.copy_from_slice(src);
            }

            self.skip(copy_current);
            total_done += copy_current;
            remaining_total -= copy_current;

            if self.is_finished() || remaining_total == 0 {
                break;
            }
        }

        Ok(())
    }
}
