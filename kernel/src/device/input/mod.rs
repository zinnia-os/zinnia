use crate::{
    clock,
    device::Device as CharacterDevice,
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
        inode::{MknodTarget, Mode},
    },
};
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec,
};
use core::sync::atomic::{AtomicU32, Ordering};

const EVENT_QUEUE_CAPACITY: usize = 256;
static EVENT_COUNTER: AtomicU32 = AtomicU32::new(0);

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
    index: u32,
    ops: Arc<dyn EventDeviceOps>,
    readers: SpinMutex<BTreeMap<u32, Arc<SpinMutex<VecDeque<InputEvent>>>>>,
    rd_event: Event,
}

struct EventDeviceFile {
    device: Arc<EventDevice>,
    reader_id: u32,
    events: Arc<SpinMutex<VecDeque<InputEvent>>>,
}

impl EventDevice {
    pub fn new(ops: Arc<dyn EventDeviceOps>) -> Arc<Self> {
        let index = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            index,
            ops,
            readers: SpinMutex::new(BTreeMap::new()),
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

        let readers = self.readers.lock();
        for buf in readers.values() {
            let mut buf = buf.lock();
            if buf.len() >= EVENT_QUEUE_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(ev);
        }

        drop(readers);
        self.rd_event.wake_all();
        handle_console_event(ev.typ, ev.code, ev.value);
    }

    pub fn register_device(self: &Arc<Self>) -> EResult<()> {
        let name = format!("input/event{}", self.index);
        let root = devtmpfs::get_root();

        vfs::mknod(
            root.clone(),
            root,
            name.as_bytes(),
            Mode::from_bits_truncate(0o666),
            Some(MknodTarget::CharacterDevice(self.clone())),
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

impl CharacterDevice for EventDevice {
    fn open(self: Arc<Self>, _flags: OpenFlags) -> EResult<Arc<dyn FileOps>> {
        let reader_id = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let events = Arc::try_new(SpinMutex::new(VecDeque::with_capacity(
            EVENT_QUEUE_CAPACITY,
        )))?;

        self.readers.lock().insert(reader_id, events.clone());

        Ok(Arc::try_new(EventDeviceFile {
            device: self,
            reader_id,
            events,
        })?)
    }

    fn major(&self) -> u32 {
        13
    }

    fn minor(&self) -> u32 {
        64 + self.index
    }
}

impl EventDevice {
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
                let name = self.ops.name().as_bytes();
                let copy_len = core::cmp::min(size.saturating_sub(1), name.len());
                let ptr = UserPtr::<u8>::new(arg);
                for (i, b) in name[..copy_len].iter().enumerate() {
                    ptr.offset(i).write(*b).ok_or(Errno::EFAULT)?;
                }
                if size > 0 {
                    ptr.offset(copy_len).write(0).ok_or(Errno::EFAULT)?;
                }
                Ok(copy_len + 1)
            }
            // EVIOCGPHYS(len) / EVIOCGUNIQ(len)
            0x07 | 0x08 => {
                if size > 0 {
                    let mut ptr: UserPtr<u8> = UserPtr::new(arg);
                    ptr.write(0).ok_or(Errno::EFAULT)?;
                }
                Ok(0)
            }
            // EVIOCGPROP(len)
            0x09 => {
                let ptr = UserPtr::<u8>::new(arg);
                for i in 0..size {
                    ptr.offset(i).write(0).ok_or(Errno::EFAULT)?;
                }
                Ok(size)
            }
            // EVIOCGKEY | EVIOCGLED | EVIOCGSND | EVIOCGSW
            0x18..=0x1b => {
                let ptr = UserPtr::<u8>::new(arg);
                for i in 0..size {
                    ptr.offset(i).write(0).ok_or(Errno::EFAULT)?;
                }
                Ok(size)
            }
            // EVIOCGBIT(ev, len)
            0x20..=0x7f => {
                let ev = nr - 0x20;
                let mut data = vec![0u8; size];
                match ev {
                    0 => {
                        let supported = self.ops.supported_events();
                        for bit in 0..32u16 {
                            if supported & (1u32 << bit) != 0 {
                                Self::set_bit(&mut data, bit);
                            }
                        }
                    }
                    x if x == input::EV_KEY as u8 => {
                        let src = self.ops.supported_keys();
                        let n = core::cmp::min(src.len(), data.len());
                        data[..n].copy_from_slice(&src[..n]);
                    }
                    x if x == input::EV_REL as u8 => {
                        let src = self.ops.supported_rel();
                        let n = core::cmp::min(src.len(), data.len());
                        data[..n].copy_from_slice(&src[..n]);
                    }
                    _ => {}
                }
                let ptr = UserPtr::<u8>::new(arg);
                for (i, b) in data.iter().enumerate() {
                    ptr.offset(i).write(*b).ok_or(Errno::EFAULT)?;
                }
                Ok(size)
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
}

impl Drop for EventDeviceFile {
    fn drop(&mut self) {
        self.device.readers.lock().remove(&self.reader_id);
    }
}

impl FileOps for EventDeviceFile {
    fn read(&self, file: &File, buffer: &mut IovecIter, _offset: u64) -> EResult<isize> {
        let ev_size = size_of::<InputEvent>();
        if buffer.len() < ev_size {
            return Err(Errno::EINVAL);
        }

        if file.flags.lock().contains(OpenFlags::NonBlocking) {
            let mut buf = self.events.lock();
            if buf.is_empty() {
                return Err(Errno::EAGAIN);
            }
            return EventDevice::drain_events(&mut buf, buffer, ev_size);
        }

        // Wait for events if we can block.
        loop {
            let guard = self.device.rd_event.guard();
            {
                let mut buf = self.events.lock();
                if !buf.is_empty() {
                    return EventDevice::drain_events(&mut buf, buffer, ev_size);
                }
            }
            guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn ioctl(&self, file: &File, request: usize, arg: VirtAddr) -> EResult<usize> {
        self.device.ioctl(file, request, arg)
    }

    fn poll(&self, _file: &File, mask: PollFlags) -> EResult<PollFlags> {
        let mut revents = PollFlags::empty();
        if mask.contains(PollFlags::In) && !self.events.lock().is_empty() {
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

struct KeyboardState {
    shift: bool,
    ctrl: bool,
    alt: bool,
    caps_lock: bool,
}

impl KeyboardState {
    const fn new() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            caps_lock: false,
        }
    }
}

static KEYBOARD_STATE: SpinMutex<KeyboardState> = SpinMutex::new(KeyboardState::new());

fn handle_console_event(typ: u16, code: u16, value: i32) {
    if typ != input::EV_KEY {
        return;
    }

    let mut state = KEYBOARD_STATE.lock();
    match code {
        input::KEY_LEFTSHIFT | input::KEY_RIGHTSHIFT => {
            state.shift = value != 0;
            return;
        }
        input::KEY_LEFTCTRL | input::KEY_RIGHTCTRL => {
            state.ctrl = value != 0;
            return;
        }
        input::KEY_LEFTALT | input::KEY_RIGHTALT => {
            state.alt = value != 0;
            return;
        }
        input::KEY_CAPSLOCK => {
            if value == 1 {
                state.caps_lock = !state.caps_lock;
            }
            return;
        }
        _ => {}
    }

    if value == 0 {
        return;
    }

    if let Some(bytes) = special_key_bytes(code) {
        crate::device::vt::input_bytes(bytes);
        return;
    }

    let Some(byte) = key_byte(code, &state) else {
        return;
    };

    if state.alt {
        crate::device::vt::input_bytes(&[0x1b, byte]);
    } else {
        crate::device::vt::input_bytes(&[byte]);
    }
}

fn special_key_bytes(code: u16) -> Option<&'static [u8]> {
    Some(match code {
        input::KEY_UP => b"\x1b[A",
        input::KEY_DOWN => b"\x1b[B",
        input::KEY_RIGHT => b"\x1b[C",
        input::KEY_LEFT => b"\x1b[D",
        input::KEY_HOME => b"\x1b[H",
        input::KEY_END => b"\x1b[F",
        input::KEY_INSERT => b"\x1b[2~",
        input::KEY_DELETE => b"\x1b[3~",
        input::KEY_PAGEUP => b"\x1b[5~",
        input::KEY_PAGEDOWN => b"\x1b[6~",
        input::KEY_F1 => b"\x1bOP",
        input::KEY_F2 => b"\x1bOQ",
        input::KEY_F3 => b"\x1bOR",
        input::KEY_F4 => b"\x1bOS",
        input::KEY_F5 => b"\x1b[15~",
        input::KEY_F6 => b"\x1b[17~",
        input::KEY_F7 => b"\x1b[18~",
        input::KEY_F8 => b"\x1b[19~",
        input::KEY_F9 => b"\x1b[20~",
        input::KEY_F10 => b"\x1b[21~",
        input::KEY_F11 => b"\x1b[23~",
        input::KEY_F12 => b"\x1b[24~",
        _ => return None,
    })
}

fn key_byte(code: u16, state: &KeyboardState) -> Option<u8> {
    if state.ctrl {
        return ctrl_key_byte(code);
    }

    if let Some(lower) = letter_key_byte(code) {
        let upper = state.shift ^ state.caps_lock;
        return Some(if upper {
            lower.to_ascii_uppercase()
        } else {
            lower
        });
    }

    match code {
        input::KEY_1 => Some(if state.shift { b'!' } else { b'1' }),
        input::KEY_2 => Some(if state.shift { b'@' } else { b'2' }),
        input::KEY_3 => Some(if state.shift { b'#' } else { b'3' }),
        input::KEY_4 => Some(if state.shift { b'$' } else { b'4' }),
        input::KEY_5 => Some(if state.shift { b'%' } else { b'5' }),
        input::KEY_6 => Some(if state.shift { b'^' } else { b'6' }),
        input::KEY_7 => Some(if state.shift { b'&' } else { b'7' }),
        input::KEY_8 => Some(if state.shift { b'*' } else { b'8' }),
        input::KEY_9 => Some(if state.shift { b'(' } else { b'9' }),
        input::KEY_0 => Some(if state.shift { b')' } else { b'0' }),
        input::KEY_MINUS => Some(if state.shift { b'_' } else { b'-' }),
        input::KEY_EQUAL => Some(if state.shift { b'+' } else { b'=' }),
        input::KEY_LEFTBRACE => Some(if state.shift { b'{' } else { b'[' }),
        input::KEY_RIGHTBRACE => Some(if state.shift { b'}' } else { b']' }),
        input::KEY_BACKSLASH => Some(if state.shift { b'|' } else { b'\\' }),
        input::KEY_SEMICOLON => Some(if state.shift { b':' } else { b';' }),
        input::KEY_APOSTROPHE => Some(if state.shift { b'"' } else { b'\'' }),
        input::KEY_GRAVE => Some(if state.shift { b'~' } else { b'`' }),
        input::KEY_COMMA => Some(if state.shift { b'<' } else { b',' }),
        input::KEY_DOT => Some(if state.shift { b'>' } else { b'.' }),
        input::KEY_SLASH => Some(if state.shift { b'?' } else { b'/' }),
        input::KEY_KPSLASH => Some(b'/'),
        input::KEY_KPASTERISK => Some(b'*'),
        input::KEY_KPMINUS => Some(b'-'),
        input::KEY_KPPLUS => Some(b'+'),
        input::KEY_KPENTER | input::KEY_ENTER => Some(b'\n'),
        input::KEY_BACKSPACE => Some(0x7f),
        input::KEY_TAB => Some(b'\t'),
        input::KEY_ESC => Some(0x1b),
        input::KEY_SPACE => Some(b' '),
        input::KEY_KP0 => Some(b'0'),
        input::KEY_KP1 => Some(b'1'),
        input::KEY_KP2 => Some(b'2'),
        input::KEY_KP3 => Some(b'3'),
        input::KEY_KP4 => Some(b'4'),
        input::KEY_KP5 => Some(b'5'),
        input::KEY_KP6 => Some(b'6'),
        input::KEY_KP7 => Some(b'7'),
        input::KEY_KP8 => Some(b'8'),
        input::KEY_KP9 => Some(b'9'),
        input::KEY_KPDOT => Some(b'.'),
        _ => None,
    }
}

fn ctrl_key_byte(code: u16) -> Option<u8> {
    if let Some(lower) = letter_key_byte(code) {
        return Some(lower - b'a' + 1);
    }

    match code {
        input::KEY_LEFTBRACE => Some(0x1b),
        input::KEY_BACKSLASH => Some(0x1c),
        input::KEY_RIGHTBRACE => Some(0x1d),
        input::KEY_6 => Some(0x1e),
        input::KEY_MINUS => Some(0x1f),
        input::KEY_2 => Some(0x00),
        input::KEY_8 => Some(0x7f),
        input::KEY_KPENTER | input::KEY_ENTER => Some(b'\n'),
        input::KEY_BACKSPACE => Some(0x7f),
        input::KEY_TAB => Some(b'\t'),
        input::KEY_ESC => Some(0x1b),
        _ => None,
    }
}

fn letter_key_byte(code: u16) -> Option<u8> {
    Some(match code {
        input::KEY_A => b'a',
        input::KEY_B => b'b',
        input::KEY_C => b'c',
        input::KEY_D => b'd',
        input::KEY_E => b'e',
        input::KEY_F => b'f',
        input::KEY_G => b'g',
        input::KEY_H => b'h',
        input::KEY_I => b'i',
        input::KEY_J => b'j',
        input::KEY_K => b'k',
        input::KEY_L => b'l',
        input::KEY_M => b'm',
        input::KEY_N => b'n',
        input::KEY_O => b'o',
        input::KEY_P => b'p',
        input::KEY_Q => b'q',
        input::KEY_R => b'r',
        input::KEY_S => b's',
        input::KEY_T => b't',
        input::KEY_U => b'u',
        input::KEY_V => b'v',
        input::KEY_W => b'w',
        input::KEY_X => b'x',
        input::KEY_Y => b'y',
        input::KEY_Z => b'z',
        _ => return None,
    })
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
