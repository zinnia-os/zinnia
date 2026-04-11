use crate::{
    arch,
    memory::{MemoryObject, PagedMemoryObject, VirtAddr, pmm::KernelAlloc, virt::VmFlags},
    process::signal::Signal,
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

/// Generic page fault handler for MMU-generated faults.
pub fn handler(info: &PageFaultInfo) -> bool {
    let task = Scheduler::get_current();
    let proc = task.get_process();
    let mut space = proc.address_space.lock();

    // Check if the current address space has a theoretical mapping at the faulting address.
    let page_size = arch::virt::get_page_size();
    let faulty_page = info.addr.value() / arch::virt::get_page_size();
    if let Some(mapped) = {
        let mappings = &space.mappings;
        mappings
            .iter()
            .find(|x| faulty_page >= x.start_page && faulty_page < x.end_page)
            .cloned()
    } {
        // Do copy on write.
        let mut map_flags = mapped.get_flags();
        let wants_cow = map_flags.contains(VmFlags::CopyOnWrite);
        let mut mapped_obj = mapped.object.clone();

        if wants_cow && info.caused_by_write {
            map_flags &= !VmFlags::CopyOnWrite;

            let num_pages = mapped.end_page - mapped.start_page;
            let region_len = NonZeroUsize::new(num_pages * page_size).unwrap();
            let region_addr = (mapped.start_page * page_size).into();
            let region_offset = (mapped.offset_page * page_size) as _;

            if Arc::strong_count(&mapped.object) == 1 {
                space
                    .map_object(
                        mapped.object.clone(),
                        region_addr,
                        region_len,
                        map_flags,
                        region_offset,
                    )
                    .unwrap();
                mapped_obj = mapped.object.clone();
            } else {
                let new_obj: Arc<dyn MemoryObject> = Arc::new(PagedMemoryObject::new_phys());

                // Copy the data from the old mapping into the new private object.
                new_obj.copy(
                    mapped.offset_page * page_size,
                    mapped.object.as_ref(),
                    mapped.offset_page * page_size,
                    page_size * num_pages,
                );

                space
                    .map_object(
                        new_obj.clone(),
                        region_addr,
                        region_len,
                        map_flags,
                        region_offset,
                    )
                    .unwrap();
                mapped_obj = new_obj;
            }
        } else if wants_cow {
            map_flags &= !VmFlags::Write;
        }

        if let Some(phys) =
            mapped_obj.try_get_page((faulty_page - mapped.start_page) + mapped.offset_page)
        {
            // If we get here, the accessed address is valid. Map it in the actual page table and return.
            space
                .table
                .map_single::<KernelAlloc>(info.addr, phys, map_flags)
                .expect("Failed to map a demand-loaded page");
            return true;
        }
    }

    // If there is no resolvable mapping here, but we were trying to copy from/to user memory,
    // fault gracefully via the user access region fixup.
    let uar = task.uar.load(Ordering::Relaxed);
    if !uar.is_null() {
        return false;
    }

    if info.caused_by_user {
        // Force SIGSEGV to the faulting user process. Using force_signal ensures the signal
        // cannot be masked or caught in a loop (handler is reset to SIG_DFL).
        let task = crate::sched::Scheduler::get_current();
        crate::process::signal::force_signal_to_thread(&task, Signal::SIGSEGV);
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
