use crate::{
    device,
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::PROCESS_STAGE,
    uapi::{self, termios::winsize},
    vfs::{File, file::FileOps, fs::devtmpfs::DEVTMPFS_STAGE, inode::Mode},
};
use alloc::sync::Arc;

#[derive(Debug)]
struct Console;

impl FileOps for Console {
    fn read(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        // TODO: Read into buffer
        Err(Errno::EBADF)
    }

    fn write(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        // TODO: Clear buffer
        Err(Errno::EBADF)
    }

    fn ioctl(&self, _: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        match request as _ {
            uapi::ioctls::TIOCGWINSZ => {
                let mut arg = UserPtr::new(arg);
                arg.write(winsize {
                    ws_row: 25,
                    ws_col: 80,
                    ..Default::default()
                })
                .ok_or(Errno::EFAULT)?;
            }
            _ => return Err(Errno::ENOSYS),
        }
        Ok(0)
    }
}

#[initgraph::task(
    name = "generic.device.console",
    depends = [PROCESS_STAGE, DEVTMPFS_STAGE]
)]
fn KMSG_STAGE() {
    device::register_char_node(
        b"kmsg",
        device::make_shared(Arc::new(Console), 1, 11),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create /dev/kmsg");
}
