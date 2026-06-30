use crate::{
    clock,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    uapi::epoll::{EPOLLERR, EPOLLHUP, EPOLLMSG, EPOLLONESHOT, epoll_data, epoll_event},
    util::{
        event::{Event, ObserverHandle},
        mutex::spin::SpinMutex,
    },
    vfs::{
        File,
        file::{FileOps, PollEventSet, PollFlags},
    },
};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::time::Duration;

const MAX_NEST: usize = 4;

pub struct EpollFile {
    ready: Arc<Event>,
    items: SpinMutex<BTreeMap<i32, EpollItem>>,
}

struct EpollItem {
    // Drop observers before the file that owns their source events.
    _observers: Vec<ObserverHandle>,
    file: Arc<File>,
    interest: u32,
    data: u64,
    disabled: bool,
}

impl EpollFile {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(Event::new()),
            items: SpinMutex::new(BTreeMap::new()),
        }
    }

    pub fn add(&self, fd: i32, file: Arc<File>, events: u32, data: u64) -> EResult<()> {
        self.check_no_loop(&file, MAX_NEST)?;
        let interest = events | EPOLLERR | EPOLLHUP;
        let observers = self.observe(&file, interest);

        {
            let mut items = self.items.lock();
            if items.contains_key(&fd) {
                return Err(Errno::EEXIST);
            }
            items.insert(
                fd,
                EpollItem {
                    _observers: observers,
                    file,
                    interest,
                    data,
                    disabled: false,
                },
            );
        }
        self.ready.wake_all();
        Ok(())
    }

    pub fn modify(&self, fd: i32, events: u32, data: u64) -> EResult<()> {
        let file = {
            let items = self.items.lock();
            items.get(&fd).ok_or(Errno::ENOENT)?.file.clone()
        };
        let interest = events | EPOLLERR | EPOLLHUP;
        let observers = self.observe(&file, interest);

        {
            let mut items = self.items.lock();
            let item = items.get_mut(&fd).ok_or(Errno::ENOENT)?;
            item._observers = observers;
            item.interest = interest;
            item.data = data;
            item.disabled = false;
        }
        self.ready.wake_all();
        Ok(())
    }

    pub fn delete(&self, fd: i32) -> EResult<()> {
        self.items.lock().remove(&fd).ok_or(Errno::ENOENT)?;
        Ok(())
    }

    fn observe(&self, file: &File, interest: u32) -> Vec<ObserverHandle> {
        let weak = Arc::downgrade(&self.ready);
        file.ops
            .poll_events(file, poll_flags_from_epoll(interest))
            .iter()
            .map(|ev| ev.add_observer(weak.clone()))
            .collect()
    }

    fn check_no_loop(&self, file: &Arc<File>, depth: usize) -> EResult<()> {
        let Ok(child) = Arc::downcast::<EpollFile>(file.ops.clone()) else {
            return Ok(());
        };
        if core::ptr::eq(Arc::as_ptr(&child), self) || depth == 0 {
            return Err(Errno::ELOOP);
        }
        let files: Vec<Arc<File>> = child
            .items
            .lock()
            .values()
            .map(|i| i.file.clone())
            .collect();
        for file in &files {
            self.check_no_loop(file, depth - 1)?;
        }
        Ok(())
    }

    pub fn wait(
        &self,
        maxevents: usize,
        deadline: Option<Duration>,
        nonblocking: bool,
    ) -> EResult<Vec<epoll_event>> {
        let timeout = deadline.map(clock::timeout_at);
        let mut out: Vec<epoll_event> = Vec::with_capacity(maxevents);

        loop {
            let guard = (!nonblocking).then(|| self.ready.guard());

            out.clear();
            let snapshot: Vec<(i32, u32, u64, bool, Arc<File>)> = self
                .items
                .lock()
                .iter()
                .map(|(fd, it)| (*fd, it.interest, it.data, it.disabled, it.file.clone()))
                .collect();

            let mut oneshot_fired: Vec<i32> = Vec::new();
            for (fd, interest, data, disabled, file) in &snapshot {
                if out.len() >= maxevents {
                    break;
                }
                if *disabled {
                    continue;
                }
                let mask = poll_flags_from_epoll(*interest);
                let revents = file.ops.poll(file, mask).unwrap_or(PollFlags::Err);
                let reported = epoll_bits_from_poll(revents) & *interest;
                if reported != 0 {
                    out.push(epoll_event {
                        events: reported,
                        data: epoll_data { num_u64: *data },
                    });
                    if *interest & EPOLLONESHOT != 0 {
                        oneshot_fired.push(*fd);
                    }
                }
            }

            if !oneshot_fired.is_empty() {
                let mut items = self.items.lock();
                for fd in oneshot_fired {
                    if let Some(item) = items.get_mut(&fd) {
                        item.disabled = true;
                    }
                }
            }

            if !out.is_empty() || nonblocking || timeout.as_ref().is_some_and(|g| g.expired()) {
                return Ok(out);
            }

            if let Some(g) = &guard {
                g.wait();
            }
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }
}

impl FileOps for EpollFile {
    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        if !mask.intersects(PollFlags::Read) {
            return Ok(PollFlags::empty());
        }
        let snapshot: Vec<(u32, bool, Arc<File>)> = self
            .items
            .lock()
            .values()
            .map(|it| (it.interest, it.disabled, it.file.clone()))
            .collect();
        for (interest, disabled, file) in snapshot {
            if disabled {
                continue;
            }
            let m = poll_flags_from_epoll(interest);
            let revents = file.ops.poll(&file, m).unwrap_or(PollFlags::Err);
            if epoll_bits_from_poll(revents) & interest != 0 {
                return Ok(PollFlags::In);
            }
        }
        Ok(PollFlags::empty())
    }

    fn poll_events(&self, _file: &File, _mask: PollFlags) -> PollEventSet<'_> {
        PollEventSet::one(self.ready.as_ref())
    }
}

const fn poll_flags_from_epoll(events: u32) -> PollFlags {
    let bits = (events & 0x07FF) & !EPOLLMSG;
    PollFlags::from_bits_truncate(bits as i16)
}

const fn epoll_bits_from_poll(revents: PollFlags) -> u32 {
    (revents.bits() as u16) as u32
}
