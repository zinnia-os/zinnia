use crate::{
    arch,
    memory::{
        MemoryObject, PagedMemoryObject, VirtAddr,
        pmm::KernelAlloc,
        virt::{VmCacheType, VmFlags, shootdown},
    },
    process::signal::{self, SigInfoData, Signal},
    sched::Scheduler,
};
use alloc::sync::Arc;
use core::{num::NonZeroUsize, sync::atomic::Ordering};

/// Abstract information about a page fault.
#[derive(Debug)]
pub struct PageFaultInfo {
    /// The instruction pointer address at the point of the page fault.
    pub ip: VirtAddr,
    /// The address that was attempted to access.
    pub addr: VirtAddr,
    /// If set, the fault was caused by a user access.
    pub caused_by_user: bool,
    /// If set, the fault was caused by a write.
    pub caused_by_write: bool,
    /// If set, the fault was caused by an instruction fetch.
    pub caused_by_fetch: bool,
    /// If set, the fault occured in a present page.
    pub page_was_present: bool,
}

struct ResolvedFault {
    /// The backing object whose page must be paged in.
    object: Arc<dyn MemoryObject>,
    /// The page index within `object`.
    object_page: usize,
    /// The PTE flags to install.
    map_flags: VmFlags,
    /// The caching mode to install.
    cache: VmCacheType,
    /// Whether this is a copy-on-write *write* fault.
    cow_write: bool,
}

/// Generic page fault handler for MMU-generated faults.
pub fn handler(info: &PageFaultInfo) -> bool {
    let task = Scheduler::get_current();
    let page_size = arch::virt::get_page_size();
    let faulty_page = info.addr.value() / page_size;

    let resolved = {
        let space = task.address_space.lock();
        let Some(mapped) = space
            .mappings
            .iter()
            .find(|x| faulty_page >= x.start_page && faulty_page < x.end_page)
            .cloned()
        else {
            return signal_or_panic(info);
        };

        let mut map_flags = mapped.get_flags();
        let wants_cow = map_flags.contains(VmFlags::CopyOnWrite);
        let access_allowed = if info.caused_by_write {
            map_flags.contains(VmFlags::Write)
        } else if info.caused_by_fetch {
            map_flags.contains(VmFlags::Exec)
        } else {
            map_flags.intersects(VmFlags::Read | VmFlags::Write)
        };
        if !access_allowed {
            return signal_or_panic(info);
        }

        let cow_write = wants_cow && info.caused_by_write;
        if cow_write {
            map_flags &= !VmFlags::CopyOnWrite;
        } else if wants_cow {
            map_flags &= !VmFlags::Write;
        }

        ResolvedFault {
            object: mapped.object.clone(),
            object_page: (faulty_page - mapped.start_page) + mapped.offset_page,
            map_flags,
            cache: mapped.cache,
            cow_write,
        }
    };

    let Some(src_phys) = resolved.object.try_get_page(resolved.object_page) else {
        return signal_or_panic(info);
    };

    // For copy-on-write writes, materialise a private copy off the source page.
    let (cow_object, install_phys) = if resolved.cow_write {
        let new_obj: Arc<dyn MemoryObject> = Arc::new(PagedMemoryObject::new_phys());
        new_obj.copy(
            0,
            resolved.object.as_ref(),
            resolved.object_page * page_size,
            page_size,
        );
        let Some(phys) = new_obj.try_get_page(0) else {
            return signal_or_panic(info);
        };
        (Some(new_obj), phys)
    } else {
        (None, src_phys)
    };

    let mut space = task.address_space.lock();
    let still_valid = space
        .mappings
        .iter()
        .find(|m| faulty_page >= m.start_page && faulty_page < m.end_page)
        .is_some_and(|m| {
            let same_offset = (faulty_page - m.start_page) + m.offset_page == resolved.object_page;
            let flags = m.get_flags();
            let wants_cow_now = flags.contains(VmFlags::CopyOnWrite);
            let cow_write_now = wants_cow_now && info.caused_by_write;
            let allowed = if info.caused_by_write {
                flags.contains(VmFlags::Write) || wants_cow_now
            } else if info.caused_by_fetch {
                flags.contains(VmFlags::Exec)
            } else {
                flags.intersects(VmFlags::Read | VmFlags::Write)
            };
            Arc::ptr_eq(&m.object, &resolved.object)
                && same_offset
                && allowed
                && cow_write_now == resolved.cow_write
        });

    if !still_valid {
        // The region was unmapped/remapped during the page-in.
        return true;
    }

    if let Some(new_obj) = cow_object {
        // Replace the shadowed page with the private copy (unmaps the old PTE
        // without a shootdown), then flush the stale read-only entry below.
        space
            .map_object(
                new_obj,
                (faulty_page * page_size).into(),
                NonZeroUsize::new(page_size).unwrap(),
                resolved.map_flags,
                0,
            )
            .unwrap();
    }

    space
        .table
        .map_single::<KernelAlloc>(info.addr, install_phys, resolved.map_flags, resolved.cache)
        .expect("Failed to map a demand-loaded page");

    if resolved.cow_write {
        let table = space.table.clone();
        drop(space);
        shootdown::submit_shootdown(&table, faulty_page * page_size, page_size);
    }
    true
}

fn signal_or_panic(info: &PageFaultInfo) -> bool {
    let task = Scheduler::get_current();

    // If there is no resolvable mapping here, but we were trying to copy from/to user memory,
    // fault gracefully via the user access region fixup.
    let uar = task.uar.load(Ordering::Relaxed);
    if !uar.is_null() {
        return false;
    }

    if info.caused_by_user {
        // Force SIGSEGV to the faulting user process. Using force_signal ensures the signal
        // cannot be masked or caught in a loop (handler is reset to SIG_DFL).
        let code = if info.page_was_present {
            crate::uapi::signal::SEGV_ACCERR
        } else {
            crate::uapi::signal::SEGV_MAPERR
        };
        let sig_info = SigInfoData {
            code: code as i32,
            addr: info.addr.value(),
            ..Default::default()
        };
        let proc = task.get_process();
        warn!(
            "segfault in {} (pid {}): {} {} page at {:#x} (IP {:#x})",
            proc.get_name(),
            proc.get_pid(),
            if info.caused_by_write {
                "write to"
            } else if info.caused_by_fetch {
                "execute on"
            } else {
                "read from"
            },
            if info.page_was_present {
                "present"
            } else {
                "non-present"
            },
            info.addr.value(),
            info.ip.value(),
        );
        signal::force_signal_to_thread(&task, Signal::SigSegv, sig_info);
        return true; // Will be delivered on return to userspace.
    }

    // If any other attempt to recover has failed, we made a mistake.
    panic!(
        "Kernel caused an unrecoverable page fault. Attempted to {} a {} page at {:#x} (IP: {:#x})",
        if info.caused_by_write {
            "write to"
        } else if info.caused_by_fetch {
            "execute on"
        } else {
            "read from"
        },
        if info.page_was_present {
            "present"
        } else {
            "non-present"
        },
        info.addr.0,
        info.ip.0
    );
}
