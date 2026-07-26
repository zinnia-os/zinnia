use crate::error::NvmeError;
use zinnia::{
    alloc::vec::Vec,
    device::block::BlockSegment,
    memory::{AllocFlags, OwnedPhysPages},
};

/// Owns a PRP list page for the lifetime of a command.
pub struct PrpList {
    _page: Option<OwnedPhysPages>,
}

impl PrpList {
    const fn empty() -> Self {
        Self { _page: None }
    }
}

pub fn build_prps(segments: &[BlockSegment]) -> Result<(u64, u64, PrpList), NvmeError> {
    let page_size = zinnia::arch::virt::get_page_size();
    let first = segments.first().ok_or(NvmeError::MmioFailed)?;
    let prp1 = first.phys();

    // Enumerate the base of every page the transfer touches in order.
    let mut rest_pages = Vec::new();
    let mut first_page = true;
    for seg in segments {
        if seg.is_empty() {
            continue;
        }
        let start = seg.phys().value();
        let end = start + seg.len();
        let mut page = start & !(page_size - 1);
        while page < end {
            if first_page {
                first_page = false;
            } else {
                rest_pages.push(page as u64);
            }
            page += page_size;
        }
    }

    if rest_pages.is_empty() {
        return Ok((prp1.into(), 0, PrpList::empty()));
    }

    if rest_pages.len() == 1 {
        return Ok((prp1.into(), rest_pages[0], PrpList::empty()));
    }

    // We don't want to chain across pages.
    let entries_per_page = page_size / size_of::<u64>();
    if rest_pages.len() > entries_per_page {
        return Err(NvmeError::AllocationFailed);
    }

    let list =
        OwnedPhysPages::new(1, AllocFlags::empty()).map_err(|_| NvmeError::AllocationFailed)?;
    let ptr = list.as_hhdm::<u64>();
    for (i, entry) in rest_pages.iter().enumerate() {
        unsafe { ptr.add(i).write(*entry) };
    }

    let prp2 = list.phys().into();
    Ok((prp1.into(), prp2, PrpList { _page: Some(list) }))
}
