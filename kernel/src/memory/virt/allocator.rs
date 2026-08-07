use crate::{
    arch,
    memory::VirtAddr,
    posix::errno::{EResult, Errno},
    util::{align_up, divide_up},
};
use alloc::collections::btree_set::BTreeSet;
use core::{num::NonZeroUsize, ops::Bound};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct VirtRange {
    start_page: usize,
    end_page: usize,
}

impl VirtRange {
    fn probe_min(start_page: usize) -> Self {
        Self {
            start_page,
            end_page: 0,
        }
    }

    fn probe_max(start_page: usize) -> Self {
        Self {
            start_page,
            end_page: usize::MAX,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtualAllocator {
    start_page: usize,
    end_page: usize,
    next_fit: usize,
    allocated: BTreeSet<VirtRange>,
}

impl VirtualAllocator {
    pub fn new(start: VirtAddr, end: VirtAddr) -> EResult<Self> {
        let page_size = arch::virt::get_page_size();
        if !start.value().is_multiple_of(page_size) || !end.value().is_multiple_of(page_size) {
            return Err(Errno::EINVAL);
        }

        let start_page = start.value() / page_size;
        let end_page = end.value() / page_size;
        if start_page >= end_page {
            return Err(Errno::EINVAL);
        }

        Ok(Self {
            start_page,
            end_page,
            next_fit: start_page,
            allocated: BTreeSet::new(),
        })
    }

    pub fn allocate(&mut self, len: NonZeroUsize) -> EResult<VirtAddr> {
        let page_size = arch::virt::get_page_size();

        let hint = self.next_fit.clamp(self.start_page, self.end_page);
        let virt = match self.find_free_from((hint * page_size).into(), len) {
            Ok(virt) => virt,
            Err(_) => self.find_free_from((self.start_page * page_size).into(), len)?,
        };

        let range = self.range_from_addr(virt, len)?;
        self.allocated.insert(range);
        self.next_fit = range.end_page;
        Ok(virt)
    }

    pub fn allocate_from(&mut self, base: VirtAddr, len: NonZeroUsize) -> EResult<VirtAddr> {
        let virt = self.find_free_from(base, len)?;
        self.reserve(virt, len)?;
        Ok(virt)
    }

    pub fn find_free(&self, len: NonZeroUsize) -> EResult<VirtAddr> {
        self.find_free_from((self.start_page * arch::virt::get_page_size()).into(), len)
    }

    pub fn find_free_from(&self, base: VirtAddr, len: NonZeroUsize) -> EResult<VirtAddr> {
        let page_size = arch::virt::get_page_size();
        let page_count = divide_up(len.get(), page_size);
        let base = align_up(base.value(), page_size) / page_size;
        let mut candidate = self.start_page.max(base);

        let lower = match self
            .allocated
            .range(..=VirtRange::probe_max(candidate))
            .next_back()
        {
            Some(pred) => Bound::Included(*pred),
            None => Bound::Unbounded,
        };

        for range in self.allocated.range((lower, Bound::Unbounded)) {
            if range.end_page <= candidate {
                continue;
            }

            let candidate_end = candidate.checked_add(page_count).ok_or(Errno::ENOMEM)?;
            if candidate_end <= range.start_page {
                break;
            }

            candidate = range.end_page;
        }

        let Some(candidate_end) = candidate.checked_add(page_count) else {
            return Err(Errno::ENOMEM);
        };
        if candidate_end > self.end_page {
            return Err(Errno::ENOMEM);
        }

        Ok((candidate * page_size).into())
    }

    pub fn reserve(&mut self, addr: VirtAddr, len: NonZeroUsize) -> EResult<()> {
        let range = self.range_from_addr(addr, len)?;

        if let Some(pred) = self
            .allocated
            .range(..VirtRange::probe_min(range.start_page))
            .next_back()
        {
            if pred.end_page > range.start_page {
                return Err(Errno::ENOMEM);
            }
        }
        if let Some(succ) = self
            .allocated
            .range(VirtRange::probe_min(range.start_page)..)
            .next()
        {
            if succ.start_page < range.end_page {
                return Err(Errno::ENOMEM);
            }
        }

        self.allocated.insert(range);
        Ok(())
    }

    pub fn release(&mut self, addr: VirtAddr, len: NonZeroUsize) -> EResult<()> {
        let range = self.range_from_addr(addr, len)?;
        self.release_range(range);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.allocated.clear();
        self.next_fit = self.start_page;
    }

    pub fn len(&self) -> usize {
        self.allocated.len()
    }

    fn range_from_addr(&self, addr: VirtAddr, len: NonZeroUsize) -> EResult<VirtRange> {
        let page_size = arch::virt::get_page_size();
        if !addr.value().is_multiple_of(page_size) {
            return Err(Errno::EINVAL);
        }
        if addr.value().checked_add(len.get()).is_none() {
            return Err(Errno::ENOMEM);
        }

        let start_page = addr.value() / page_size;
        let page_count = divide_up(len.get(), page_size);
        let end_page = start_page.checked_add(page_count).ok_or(Errno::ENOMEM)?;

        if start_page < self.start_page || end_page > self.end_page {
            return Err(Errno::ENOMEM);
        }

        Ok(VirtRange {
            start_page,
            end_page,
        })
    }

    fn release_range(&mut self, range: VirtRange) {
        let overlapping = self
            .allocated
            .range(..VirtRange::probe_min(range.end_page))
            .rev()
            .take_while(|other| other.end_page > range.start_page)
            .copied()
            .collect::<alloc::vec::Vec<_>>();

        for old in overlapping {
            self.allocated.remove(&old);

            if old.start_page < range.start_page {
                self.allocated.insert(VirtRange {
                    start_page: old.start_page,
                    end_page: range.start_page,
                });
            }

            if range.end_page < old.end_page {
                self.allocated.insert(VirtRange {
                    start_page: range.end_page,
                    end_page: old.end_page,
                });
            }
        }
    }
}

pub fn kernel_map_end() -> VirtAddr {
    let page_size = arch::virt::get_page_size();
    align_up(usize::MAX - page_size, page_size).into()
}
