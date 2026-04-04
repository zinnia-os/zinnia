use crate::{
    log::GLOBAL_LOGGERS,
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::PROCESS_STAGE,
    uapi::{self, termios::winsize},
    vfs::{
        File,
        file::FileOps,
        fs::devtmpfs::{self, DEVTMPFS_STAGE},
        inode::Mode,
    },
};
use alloc::sync::Arc;
use core::fmt::Write;

#[derive(Debug)]
struct Console;

impl FileOps for Console {
    fn read(&self, _: &File, _: &mut IovecIter, _: u64) -> EResult<isize> {
        Err(Errno::EBADF)
    }

    fn write(&self, _: &File, buffer: &mut IovecIter, _: u64) -> EResult<isize> {
        let mut writer = GLOBAL_LOGGERS.lock();
        for _ in 0..buffer.len() {
            let mut ch = [0u8];
            buffer.copy_to_slice(&mut ch)?;
            _ = writer.write_char(ch[0] as char);
        }
        Ok(buffer.len() as _)
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
fn CONSOLE_STAGE() {
    devtmpfs::register_device(
        b"console",
        crate::vfs::inode::Device::CharacterDevice(Arc::new(Console)),
        Mode::from_bits_truncate(0o666),
    )
    .expect("Unable to create console");
}
