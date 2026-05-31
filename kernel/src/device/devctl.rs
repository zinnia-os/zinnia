use crate::{
    device::{self, Device},
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    sched::Scheduler,
    util::{event::Event, mutex::spin::SpinMutex, once::Once},
    vfs::{
        File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
        fs::devtmpfs::DEVTMPFS_STAGE,
        inode::Mode,
    },
};
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Maximum number of buffered lines per reader before the oldest is dropped.
const QUEUE_CAPACITY: usize = 256;
/// Maximum number of lines buffered before any reader has connected.
const BACKLOG_CAPACITY: usize = 256;

static READER_COUNTER: AtomicU32 = AtomicU32::new(0);
static DEVCTL: Once<Arc<DevCtl>> = Once::new();
static DEVCTL_READY: AtomicBool = AtomicBool::new(false);

/// The global `/dev/devctl` device.
struct DevCtl {
    readers: SpinMutex<BTreeMap<u32, Arc<SpinMutex<VecDeque<Vec<u8>>>>>>,
    /// Lines emitted before any reader connects (so boot-time nodes aren't lost).
    backlog: SpinMutex<VecDeque<Vec<u8>>>,
    had_reader: AtomicBool,
    rd_event: Event,
}

/// A single open handle on `/dev/devctl`.
struct DevCtlFile {
    device: Arc<DevCtl>,
    reader_id: u32,
    queue: Arc<SpinMutex<VecDeque<Vec<u8>>>>,
}

impl DevCtl {
    fn new() -> Self {
        Self {
            readers: SpinMutex::new(BTreeMap::new()),
            backlog: SpinMutex::new(VecDeque::new()),
            had_reader: AtomicBool::new(false),
            rd_event: Event::new(),
        }
    }

    /// Enqueue a single notification line and wake any blocked readers.
    fn enqueue(&self, line: Vec<u8>) {
        let readers = self.readers.lock();
        if readers.is_empty() {
            drop(readers);
            // No reader yet: buffer until the first one connects, unless one has
            // already come and gone (then drop, like FreeBSD without devd).
            if !self.had_reader.load(Ordering::Acquire) {
                let mut backlog = self.backlog.lock();
                if backlog.len() >= BACKLOG_CAPACITY {
                    backlog.pop_front();
                }
                backlog.push_back(line);
            }
            return;
        }

        for buf in readers.values() {
            let mut buf = buf.lock();
            if buf.len() >= QUEUE_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(line.clone());
        }
        drop(readers);
        self.rd_event.wake_all();
    }
}

impl Device for DevCtl {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        let reader_id = READER_COUNTER.fetch_add(1, Ordering::Relaxed);
        let queue = Arc::try_new(SpinMutex::new(VecDeque::new()))?;

        // On the first ever open, drain the boot backlog into this reader.
        if !self.had_reader.swap(true, Ordering::AcqRel) {
            let mut backlog = self.backlog.lock();
            let mut q = queue.lock();
            q.extend(backlog.drain(..));
            drop(q);
            drop(backlog);
        }

        self.readers.lock().insert(reader_id, queue.clone());

        Ok(Arc::try_new(DevCtlFile {
            device: self,
            reader_id,
            queue,
        })?)
    }

    fn major(&self) -> u32 {
        5
    }

    fn minor(&self) -> u32 {
        3
    }
}

impl Drop for DevCtlFile {
    fn drop(&mut self) {
        self.device.readers.lock().remove(&self.reader_id);
    }
}

impl FileOps for DevCtlFile {
    fn read(&self, file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let non_blocking = file.flags.lock().contains(OpenFlags::NonBlocking);

        loop {
            let guard = self.device.rd_event.guard();
            {
                let mut queue = self.queue.lock();
                if !queue.is_empty() {
                    return Self::drain_lines(&mut queue, buffer);
                }
            }
            if non_blocking {
                return Err(Errno::EAGAIN);
            }
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn write(&self, _file: &File, _buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        Err(Errno::EBADF)
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let mut revents = PollFlags::empty();
        if mask.contains(PollFlags::In) && !self.queue.lock().is_empty() {
            revents |= PollFlags::In;
        }
        Ok(revents)
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        if mask.intersects(PollFlags::Read) {
            PollEventSet::one(&self.device.rd_event)
        } else {
            PollEventSet::new()
        }
    }
}

impl DevCtlFile {
    /// Copy as many whole lines as fit into the user buffer. Returns bytes copied.
    fn drain_lines(queue: &mut VecDeque<Vec<u8>>, buffer: &mut IovecIter) -> EResult<isize> {
        let mut total = 0isize;
        while let Some(line) = queue.front() {
            if line.len() > buffer.len() {
                // A single line larger than the whole buffer can never be read.
                if total == 0 {
                    return Err(Errno::EINVAL);
                }
                break;
            }
            let line = queue.pop_front().unwrap();
            total += buffer.copy_from_slice(&line)?;
        }
        Ok(total)
    }
}

fn format_line(typ: &str, relpath: &[u8]) -> Vec<u8> {
    let mut line = Vec::new();
    line.extend_from_slice(b"!system=DEVFS subsystem=CDEV type=");
    line.extend_from_slice(typ.as_bytes());
    line.extend_from_slice(b" cdev=");
    line.extend_from_slice(relpath);
    line.push(b'\n');
    line
}

pub fn notify_create(relpath: &[u8]) {
    if !DEVCTL_READY.load(Ordering::Acquire) {
        return;
    }
    DEVCTL.get().enqueue(format_line("CREATE", relpath));
}

pub fn notify_destroy(relpath: &[u8]) {
    if !DEVCTL_READY.load(Ordering::Acquire) {
        return;
    }
    DEVCTL.get().enqueue(format_line("DESTROY", relpath));
}

#[initgraph::task(
    name = "generic.device.devctl",
    depends = [DEVTMPFS_STAGE],
)]
pub fn DEVCTL_STAGE() {
    let dev = Arc::new(DevCtl::new());
    unsafe { DEVCTL.init(dev.clone()) };
    DEVCTL_READY.store(true, Ordering::Release);

    device::register_char_node(b"devctl", dev, Mode::from_bits_truncate(0o600))
        .expect("Unable to create /dev/devctl");
}
