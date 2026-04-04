use crate::{
    memory::{UserCStr, VirtAddr},
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::limits::PATH_MAX,
    vfs::{File, file::OpenFlags, inode::Mode},
};

pub fn module_insert(path: VirtAddr, cmdline: VirtAddr) -> EResult<()> {
    let path = UserCStr::new(path).as_vec(PATH_MAX).ok_or(Errno::EFAULT)?;
    let cmdline = UserCStr::new(cmdline).as_vec(4096).ok_or(Errno::EFAULT)?;

    let task = Scheduler::get_current();
    let proc = task.get_process();

    let ident = proc.identity.lock();
    if !ident.is_effective_superuser() {
        return Err(Errno::EPERM);
    }

    let file = File::open(
        proc.root_dir.lock().clone(),
        proc.working_dir.lock().clone(),
        &path,
        OpenFlags::Read,
        Mode::empty(),
        &ident,
    )?;

    let file_len: usize = file.inode.clone().ok_or(Errno::EBADF)?.len();
    let mut data = vec![0u8; file_len];

    file.pread_kernel(&mut data, 0)?;

    crate::module::load(&data, &cmdline)
}
