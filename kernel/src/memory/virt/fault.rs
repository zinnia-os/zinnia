use crate::memory::virt::VmFlags;
use crate::{
    arch,
    memory::{
        MemoryObject, PagedMemoryObject, VirtAddr,
        pmm::KernelAlloc,
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

/// Generic page fault handler for MMU-generated faults.
pub fn handler(info: &PageFaultInfo) -> bool {
    let task = Scheduler::get_current();
    let mut space = task.address_space.lock();

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

        let object_page = (faulty_page - mapped.start_page) + mapped.offset_page;
        let mapped_obj: Arc<dyn MemoryObject>;
        let mapped_obj_page;

        if wants_cow && info.caused_by_write {
            map_flags &= !VmFlags::CopyOnWrite;

            let new_obj: Arc<dyn MemoryObject> = Arc::new(PagedMemoryObject::new_phys());
            new_obj.copy(
                0,
                mapped.object.as_ref(),
                object_page * page_size,
                page_size,
            );

            space
                .map_object(
                    new_obj.clone(),
                    (faulty_page * page_size).into(),
                    NonZeroUsize::new(page_size).unwrap(),
                    map_flags,
                    0,
                )
                .unwrap();
            mapped_obj = new_obj;
            mapped_obj_page = 0;
        } else if wants_cow {
            map_flags &= !VmFlags::Write;
            mapped_obj = mapped.object.clone();
            mapped_obj_page = object_page;
        } else {
            mapped_obj = mapped.object.clone();
            mapped_obj_page = object_page;
        }

        if let Some(phys) = mapped_obj.try_get_page(mapped_obj_page) {
            // If we get here, the accessed address is valid. Map it in the actual page table and return.
            space
                .table
                .map_single::<KernelAlloc>(info.addr, phys, map_flags)
                .expect("Failed to map a demand-loaded page");
            return true;
        }
    }

    signal_or_panic(info)
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
