use crate::{
    posix::errno::{EResult, Errno},
    uapi::epoll::{EPOLLERR, EPOLLHUP},
    util::mutex::spin::SpinMutex,
    vfs::{
        File,
        file::{FileOps, PollEventSet, PollFlags},
    },
};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

pub struct EpollFile {
    interest: SpinMutex<BTreeMap<i32, Registration>>,
}

pub struct Registration {
    pub events: u32,
    pub data: u64,
    pub file: Arc<File>,
}

impl EpollFile {
    pub const fn new() -> Self {
        Self {
            interest: SpinMutex::new(BTreeMap::new()),
        }
    }

    pub fn add(&self, fd: i32, file: Arc<File>, events: u32, data: u64) -> EResult<()> {
        let mut interest = self.interest.lock();
        if interest.contains_key(&fd) {
            return Err(Errno::EEXIST);
        }
        interest.insert(
            fd,
            Registration {
                events: events | EPOLLERR | EPOLLHUP,
                data,
                file,
            },
        );
        Ok(())
    }

    pub fn modify(&self, fd: i32, events: u32, data: u64) -> EResult<()> {
        let mut interest = self.interest.lock();
        let reg = interest.get_mut(&fd).ok_or(Errno::ENOENT)?;
        reg.events = events | EPOLLERR | EPOLLHUP;
        reg.data = data;
        Ok(())
    }

    pub fn remove(&self, fd: i32) -> EResult<()> {
        let mut interest = self.interest.lock();
        interest.remove(&fd).ok_or(Errno::ENOENT)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Vec<(i32, u32, u64, Arc<File>)> {
        self.interest
            .lock()
            .iter()
            .map(|(fd, reg)| (*fd, reg.events, reg.data, reg.file.clone()))
            .collect()
    }

    pub fn disarm_oneshot(&self, fd: i32) {
        if let Some(reg) = self.interest.lock().get_mut(&fd) {
            // EPOLLERR and EPOLLHUP are always reported but the user mask becomes zero
            // so nothing will actually match until EPOLL_CTL_MOD re-arms it.
            reg.events = EPOLLERR | EPOLLHUP;
        }
    }
}

impl FileOps for EpollFile {
    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        if !mask.intersects(PollFlags::Read) {
            return Ok(PollFlags::empty());
        }
        let interest = self.interest.lock();
        for reg in interest.values() {
            let child_mask = flags_from_epoll(reg.events);
            let revents = reg
                .file
                .ops
                .poll(&reg.file, child_mask)
                .unwrap_or(PollFlags::Err);
            if !(revents & child_mask).is_empty() {
                return Ok(PollFlags::In);
            }
        }
        Ok(PollFlags::empty())
    }

    fn poll_events(&self, _file: &File, _mask: PollFlags) -> PollEventSet<'_> {
        PollEventSet::new()
    }
}

/// Convert epoll event bits to `PollFlags`, dropping metadata bits.
const fn flags_from_epoll(events: u32) -> PollFlags {
    let bits = (events & 0x07FF) & !crate::uapi::epoll::EPOLLMSG;
    PollFlags::from_bits_truncate(bits as i16)
}

impl EpollFile {
    /// Recursively collect all leaf (non-epoll) registrations watched by this epoll,
    /// so that the waiter can register on each of their readiness events.
    pub fn get_children(&self, out: &mut Vec<Arc<File>>) {
        self.get_children_recursive(out, &mut vec![self as *const _ as usize])
    }

    fn get_children_recursive(&self, out: &mut Vec<Arc<File>>, visited: &mut Vec<usize>) {
        let interest = self.interest.lock();
        for reg in interest.values() {
            if let Ok(child_epoll) = Arc::downcast::<EpollFile>(reg.file.ops.clone()) {
                let key = Arc::as_ptr(&child_epoll) as usize;
                if visited.contains(&key) {
                    continue;
                }
                visited.push(key);
                child_epoll.get_children_recursive(out, visited);
            } else {
                out.push(reg.file.clone());
            }
        }
    }
}
