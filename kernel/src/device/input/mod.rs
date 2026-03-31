use crate::{
    memory::IovecIter,
    posix::errno::EResult,
    vfs::{File, file::FileOps},
};

/// Represents a Linux evdev-compatible input device.
pub struct EventDevice {}

impl FileOps for EventDevice {
    fn read(&self, file: &File, buffer: &mut IovecIter, offset: u64) -> EResult<isize> {
        let _ = (file, buffer, offset);
        todo!()
    }
}

pub struct EventListener {}

impl EventListener {}
