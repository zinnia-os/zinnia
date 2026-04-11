use crate::{
    arch::virt::get_page_size,
    memory::{VirtAddr, virt::VmFlags},
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi,
    util::align_up,
    vfs::file::MmapFlags,
};
use core::num::NonZeroUsize;
use uapi::mman::*;

pub fn mmap(
    addr: VirtAddr,
    length: usize,
    prot: u32,
    flags: u32,
    fd: i32,
    offset: uapi::off_t,
) -> EResult<usize> {
    let flags = MmapFlags::from_bits_truncate(flags);

    // Flags must contain either MAP_PRIVATE or MAP_SHARED. Not both or none.
    if flags.intersects(MmapFlags::Shared | MmapFlags::Private) {
        if flags.contains(MmapFlags::Shared | MmapFlags::Private) {
            return Err(Errno::EINVAL);
        }
    } else {
        return Err(Errno::EINVAL);
    }

    let mut vm_prot = VmFlags::empty();
    vm_prot.set(VmFlags::Read, prot & PROT_READ != 0);
    vm_prot.set(VmFlags::Write, prot & PROT_WRITE != 0);
    vm_prot.set(VmFlags::Exec, prot & PROT_EXEC != 0);
    vm_prot.set(VmFlags::Shared, flags.contains(MmapFlags::Shared));

    let proc = Scheduler::get_current().get_process();
    let mut mmap_head = proc.mmap_head.lock();

    // If MAP_FIXED isn't specified, we must find a suitable address.
    let addr = if !flags.contains(MmapFlags::Fixed) {
        let cur = *mmap_head;
        *mmap_head = align_up((cur + length).value(), get_page_size()).into();

        cur
    } else {
        addr
    };

    let file = match flags.contains(MmapFlags::Anonymous) {
        true => None,
        false => {
            // Look up the corresponding fd.
            Some(proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?)
        }
    };
    crate::vfs::mmap(
        file.map(|x| x.file.clone()),
        &mut proc.address_space.lock(),
        addr,
        NonZeroUsize::new(length).ok_or(Errno::EINVAL)?,
        vm_prot,
        flags,
        offset,
    )
    .map(|x| x.value())
}

pub fn mprotect(addr: VirtAddr, size: usize, prot: u32) -> EResult<usize> {
    let mut vm_prot = VmFlags::empty();
    vm_prot.set(VmFlags::Read, prot & PROT_READ != 0);
    vm_prot.set(VmFlags::Write, prot & PROT_WRITE != 0);
    vm_prot.set(VmFlags::Exec, prot & PROT_EXEC != 0);

    let proc = Scheduler::get_current().get_process();
    proc.address_space.lock().protect(
        addr,
        NonZeroUsize::new(size).ok_or(Errno::EINVAL)?,
        vm_prot,
    )?;

    Ok(0)
}

pub fn munmap(addr: VirtAddr, size: usize) -> EResult<usize> {
    let proc = Scheduler::get_current().get_process();
    proc.address_space
        .lock()
        .unmap(addr, NonZeroUsize::new(size).ok_or(Errno::EINVAL)?)
        .map(|_| 0)
}
