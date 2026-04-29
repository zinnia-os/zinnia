use crate::{
    clock,
    memory::{IovecIter, VirtAddr, user::UserPtr},
    posix::errno::{EResult, Errno},
    process::Identity,
    sched::Scheduler,
    uapi::{
        self,
        input::{self, InputEvent, InputId},
        ioctls,
    },
    util::{event::Event, mutex::spin::SpinMutex},
    vfs::{
        self, File,
        file::{FileOps, OpenFlags, PollEventSet, PollFlags},
        fs::devtmpfs,
        inode::{Device, Mode},
    },
};
use alloc::{collections::vec_deque::VecDeque, sync::Arc, vec};
use core::sync::atomic::{AtomicUsize, Ordering};

const EVENT_QUEUE_CAPACITY: usize = 256;
static EVENT_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub trait EventDeviceOps: Send + Sync {
    /// Human-readable device name.
    fn name(&self) -> &str;

    /// Device identity (bus type, vendor, product, version).
    fn id(&self) -> InputId;

    /// Bitmap of supported EV_* event types.
    fn supported_events(&self) -> u32;

    /// Bitmap of supported KEY/BTN codes. Up to KEY_CNT bits.
    fn supported_keys(&self) -> &[u8] {
        &[]
    }

    /// Bitmap of supported REL_* axes. Up to REL_CNT bits.
    fn supported_rel(&self) -> &[u8] {
        &[]
    }
}

/// An evdev-compatible input device.
pub struct EventDevice {
    index: usize,
    ops: Arc<dyn EventDeviceOps>,
    event_buf: SpinMutex<VecDeque<InputEvent>>,
    rd_event: Event,
}

impl EventDevice {
    pub fn new(ops: Arc<dyn EventDeviceOps>) -> Arc<Self> {
        let index = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            index,
            ops,
            event_buf: SpinMutex::new(VecDeque::with_capacity(EVENT_QUEUE_CAPACITY)),
            rd_event: Event::new(),
        })
    }

    /// Enqueue an input event with the current timestamp and wake readers.
    pub fn report_event(&self, typ: u16, code: u16, value: i32) {
        let ns = clock::get_elapsed();
        let secs = ns / 1_000_000_000;
        let usecs = (ns % 1_000_000_000) / 1_000;

        let ev = InputEvent {
            time: uapi::time::timeval {
                tv_sec: secs as _,
                tv_usec: usecs as _,
            },
            typ,
            code,
            value,
        };

        let mut buf = self.event_buf.lock();
        if buf.len() >= EVENT_QUEUE_CAPACITY {
            buf.pop_front(); // Drop oldest event on overflow.
        }
        buf.push_back(ev);

        // Only wake after releasing the event_buf lock would be ideal,
        // but since wake_all just enqueues tasks, it's fine under the lock.
        drop(buf);
        self.rd_event.wake_all();
    }

    pub fn register_device(self: &Arc<Self>) -> EResult<()> {
        let name = format!("input/event{}", self.index);
        let root = devtmpfs::get_root();

        vfs::mknod(
            root.clone(),
            root,
            name.as_bytes(),
            Mode::from_bits_truncate(0o666),
            Some(Device::CharacterDevice(self.clone())),
            &Identity::get_kernel(),
        )
    }

    const fn set_bit(bitmap: &mut [u8], n: u16) {
        let byte = (n / 8) as usize;
        let bit = n % 8;
        if byte < bitmap.len() {
            bitmap[byte] |= 1 << bit;
        }
    }
}

impl FileOps for EventDevice {
    fn read(&self, file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let ev_size = size_of::<InputEvent>();
        if buffer.len() < ev_size {
            return Err(Errno::EINVAL);
        }

        if file.flags.lock().contains(OpenFlags::NonBlocking) {
            let mut buf = self.event_buf.lock();
            if buf.is_empty() {
                return Err(Errno::EAGAIN);
            }
            return Self::drain_events(&mut buf, buffer, ev_size);
        }

        // Wait for events if we can block.
        loop {
            let guard = self.rd_event.guard();
            {
                let mut buf = self.event_buf.lock();
                if !buf.is_empty() {
                    return Self::drain_events(&mut buf, buffer, ev_size);
                }
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn ioctl(&self, _file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        let cmd = request as u32;

        // Check if this is an evdev ioctl.
        if ioctls::ioc_type(cmd) != b'E' {
            return Err(Errno::ENOTTY);
        }

        let nr = ioctls::ioc_nr(cmd);
        let size = ioctls::ioc_size(cmd) as usize;

        match nr {
            // EVIOCGVERSION
            0x01 => {
                let mut ptr: UserPtr<u32> = UserPtr::new(arg);
                ptr.write(input::EV_VERSION).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            // EVIOCGID
            0x02 => {
                let mut ptr: UserPtr<InputId> = UserPtr::new(arg);
                ptr.write(self.ops.id()).ok_or(Errno::EFAULT)?;
                Ok(0)
            }
            // EVIOCGNAME(len)
            0x06 => {
                let name = self.ops.name();
                let copy_len = core::cmp::min(name.len(), size);
                let mut ptr: UserPtr<u8> = UserPtr::new(arg);
                ptr.write_slice(&name.as_bytes()[..copy_len])
                    .ok_or(Errno::EFAULT)?;
                // NUL-terminate if space.
                if copy_len < size {
                    let mut nul_ptr: UserPtr<u8> = UserPtr::new(arg + copy_len);
                    nul_ptr.write(0).ok_or(Errno::EFAULT)?;
                }
                Ok(copy_len)
            }
            // EVIOCGPHYS(len) / EVIOCGUNIQ(len)
            0x07 | 0x08 => {
                if size > 0 {
                    let mut ptr: UserPtr<u8> = UserPtr::new(arg);
                    ptr.write(0u8).ok_or(Errno::EFAULT)?;
                }
                Ok(0)
            }
            // EVIOCGPROP(len) — device properties bitmap (none)
            0x09 => {
                let zeros = vec![0u8; size];
                if size > 0 {
                    let mut ptr: UserPtr<u8> = UserPtr::new(arg);
                    ptr.write_slice(&zeros).ok_or(Errno::EFAULT)?;
                }
                Ok(size)
            }
            // EVIOCGKEY | EVIOCGLED | EVIOCGSND | EVIOCGSW
            0x18..=0x1b => {
                let zeros = vec![0u8; size];
                if size > 0 {
                    let mut ptr: UserPtr<u8> = UserPtr::new(arg);
                    ptr.write_slice(&zeros).ok_or(Errno::EFAULT)?;
                }
                Ok(size)
            }
            // EVIOCGBIT(ev, len)
            nr if nr >= 0x20 && nr < 0x40 => {
                let ev_type = nr - 0x20;
                let mut out = vec![0u8; size];

                match ev_type {
                    // Bitmap of supported event types.
                    0 => {
                        let supported = self.ops.supported_events();
                        for i in 0..32 {
                            if supported & (1 << i) != 0 {
                                Self::set_bit(&mut out, i);
                            }
                        }
                    }
                    // EV_KEY bitmap.
                    1 => {
                        let keys = self.ops.supported_keys();
                        let copy_len = core::cmp::min(keys.len(), size);
                        out[..copy_len].copy_from_slice(&keys[..copy_len]);
                    }
                    // EV_REL bitmap.
                    2 => {
                        let rel = self.ops.supported_rel();
                        let copy_len = core::cmp::min(rel.len(), size);
                        out[..copy_len].copy_from_slice(&rel[..copy_len]);
                    }
                    _ => {} // Unsupported event type.
                }

                let mut ptr: UserPtr<u8> = UserPtr::new(arg);
                let copy_len = core::cmp::min(out.len(), size);
                ptr.write_slice(&out[..copy_len]).ok_or(Errno::EFAULT)?;
                Ok(copy_len)
            }
            // EVIOCGRAB / EVIOCREVOKE
            0x90 | 0x91 => Ok(0),
            // EVIOCSCLOCKID
            0xa0 => Ok(0),
            _ => {
                warn!("unhandled evdev ioctl {:#x}", nr);
                Err(Errno::ENOTTY)
            }
        }
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let mut revents = PollFlags::empty();
        if mask.contains(PollFlags::In) {
            let buf = self.event_buf.lock();
            if !buf.is_empty() {
                revents |= PollFlags::In;
            }
        }
        Ok(revents)
    }

    fn poll_events(&self, _file: &File, mask: PollFlags) -> PollEventSet<'_> {
        if mask.intersects(PollFlags::Read) {
            PollEventSet::one(&self.rd_event)
        } else {
            PollEventSet::new()
        }
    }
}

impl EventDevice {
    /// Drain as many events as fit into the user buffer (in multiples of event size). Returns bytes copied.
    fn drain_events(
        buf: &mut VecDeque<InputEvent>,
        buffer: &mut IovecIter,
        ev_size: usize,
    ) -> EResult<isize> {
        let max_events = buffer.len() / ev_size;
        let n = core::cmp::min(max_events, buf.len());
        let mut total = 0isize;

        for _ in 0..n {
            if let Some(ev) = buf.pop_front() {
                let bytes = unsafe {
                    core::slice::from_raw_parts(&ev as *const InputEvent as *const u8, ev_size)
                };
                buffer.copy_from_slice(bytes)?;
                total += ev_size as isize;
            }
        }

        Ok(total)
    }
}

#[initgraph::task(
    name = "generic.device.input",
    depends = [devtmpfs::DEVTMPFS_STAGE],
)]
pub fn INPUT_STAGE() {
    let root = devtmpfs::get_root();
    vfs::mkdir(
        root.clone(),
        root,
        b"input",
        Mode::from_bits_truncate(0o755),
        &Identity::get_kernel(),
    )
    .expect("Unable to create /dev/input");
}
