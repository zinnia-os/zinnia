use crate::{
    memory::{VirtAddr, virt::VmFlags},
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi,
    vfs::file::MmapFlags,
    wrap_syscall,
};
use core::num::NonZeroUsize;
use uapi::mman::*;

#[wrap_syscall]
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

    let task = Scheduler::get_current();
    let proc = task.get_process();
    let file = match flags.contains(MmapFlags::Anonymous) {
        true => None,
        false => {
            // Look up the corresponding fd.
            Some(proc.open_files.lock().get_fd(fd).ok_or(Errno::EBADF)?)
        }
    };
    let len = NonZeroUsize::new(length).ok_or(Errno::EINVAL)?;

    let mut space = task.address_space.lock();
    let addr = if flags.contains(MmapFlags::Fixed) {
        addr
    } else {
        space.find_mmap_addr(len)?
    };

    let mapped = crate::vfs::mmap(
        file.map(|x| x.file.clone()),
        &mut space,
        addr,
        len,
        vm_prot,
        flags,
        offset,
    )?;

    let table = space.table.clone();
    drop(space);
    crate::memory::virt::shootdown::submit_shootdown(&table, mapped.value(), len.get());

    Ok(mapped.value())
}

#[wrap_syscall]
pub fn mprotect(addr: VirtAddr, size: usize, prot: u32) -> EResult<usize> {
    let mut vm_prot = VmFlags::empty();
    vm_prot.set(VmFlags::Read, prot & PROT_READ != 0);
    vm_prot.set(VmFlags::Write, prot & PROT_WRITE != 0);
    vm_prot.set(VmFlags::Exec, prot & PROT_EXEC != 0);

    let task = Scheduler::get_current();
    let size = NonZeroUsize::new(size).ok_or(Errno::EINVAL)?;
    let table = {
        let mut space = task.address_space.lock();
        space.protect(addr, size, vm_prot)?;
        space.table.clone()
    };
    crate::memory::virt::shootdown::submit_shootdown(&table, addr.value(), size.get());

    Ok(0)
}

#[wrap_syscall]
pub fn munmap(addr: VirtAddr, size: usize) -> EResult<usize> {
    let task = Scheduler::get_current();
    let size = NonZeroUsize::new(size).ok_or(Errno::EINVAL)?;
    let table = {
        let mut space = task.address_space.lock();
        space.unmap(addr, size)?;
        space.table.clone()
    };
    crate::memory::virt::shootdown::submit_shootdown(&table, addr.value(), size.get());

    Ok(0)
}

#[wrap_syscall]
pub fn madvise(_addr: VirtAddr, _size: usize, _advice: i32) -> EResult<usize> {
    // TODO: Actually do something with it.
    Ok(0)
}

#[wrap_syscall]
pub fn msync(addr: VirtAddr, size: usize, flags: i32) -> EResult<usize> {
    if flags & !(MS_ASYNC | MS_INVALIDATE | MS_SYNC) != 0 {
        return Err(Errno::EINVAL);
    }
    if flags & MS_ASYNC != 0 && flags & MS_SYNC != 0 {
        return Err(Errno::EINVAL);
    }

    let task = Scheduler::get_current();
    let size = NonZeroUsize::new(size).ok_or(Errno::EINVAL)?;
    let table = {
        let space = task.address_space.lock();
        space.sync_dirty_range(addr, size)?;
        space.table.clone()
    };
    crate::memory::virt::shootdown::submit_shootdown(&table, addr.value(), size.get());

    Ok(0)
}
