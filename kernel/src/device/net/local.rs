use alloc::sync::Weak;
use alloc::{collections::VecDeque, sync::Arc, vec, vec::Vec};
use core::cmp::min;
use core::sync::atomic::AtomicBool;

use crate::device::net::ShutdownFlags;
use crate::{
    memory::IovecIter,
    posix::errno::{EResult, Errno},
    process::signal::{Signal, send_signal_to_thread},
    sched::Scheduler,
    uapi::socket::*,
    util::{event::Event, mutex::spin::SpinMutex, ring::RingBuffer},
    vfs::{
        self, File, PathNode,
        cache::LookupFlags,
        file::{FileDescription, PollEventSet, PollFlags},
        inode::{Device, Mode, NodeOps},
    },
};

use super::{Socket, SocketOps};

const BUFFER_SIZE: usize = 0x4000;

/// Ancillary-data barrier section: fds that arrived alongside the first
/// `bytes` bytes currently at the head of the peer's recv queue.
///
/// `files` may be empty; that just means this section has no pending fds
/// to deliver. Sections are created either by a `sendmsg` with SCM_RIGHTS
/// or extended by plain data-only sends.
struct InflightSection {
    files: Vec<Arc<File>>,
    bytes: usize,
}

/// CMSG_ALIGN: align to alignof(size_t). Matches the layout mlibc emits.
const fn cmsg_align(x: usize) -> usize {
    let a = align_of::<usize>();
    (x + a - 1) & !(a - 1)
}

const fn cmsg_len(payload: usize) -> usize {
    cmsg_align(size_of::<cmsghdr>()) + payload
}

/// Record a send of `bytes` bytes (and optionally fds) into the peer's
/// inflight section queue. Data-only sends extend the trailing section's
/// byte count; a send that carries fds always starts a fresh section so
/// the reader can deliver those fds with the corresponding data range.
fn push_inflight(
    inflight: &mut VecDeque<InflightSection>,
    files: &mut Vec<Arc<File>>,
    bytes: usize,
) {
    if files.is_empty() {
        if bytes == 0 {
            return;
        }
        match inflight.back_mut() {
            Some(back) if back.files.is_empty() => back.bytes += bytes,
            _ => inflight.push_back(InflightSection {
                files: Vec::new(),
                bytes,
            }),
        }
        return;
    }

    let taken = core::mem::take(files);
    // If the tail has no data yet (e.g. a previous sendmsg that copied zero
    // bytes), merge into it rather than leaving an empty slot.
    if let Some(back) = inflight.back_mut()
        && back.bytes == 0
        && back.files.is_empty()
    {
        back.files = taken;
        back.bytes = bytes;
    } else {
        inflight.push_back(InflightSection {
            files: taken,
            bytes,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Unconnected,
    Bound,
    Listening,
    Connected,
}

struct LocalInner {
    state: State,
    sock_type: u32,
    bound_addr: Option<Vec<u8>>,
    peer: Option<Arc<LocalSocket>>,
    self_ref: Weak<LocalSocket>,
    recv_buf: RingBuffer,
    inflight: VecDeque<InflightSection>,
    backlog: VecDeque<Arc<Socket>>,
    backlog_limit: usize,
    shutdown: ShutdownFlags,
    peer_closed: bool,
    owner_cred: ucred,
}

pub struct LocalSocket {
    inner: SpinMutex<LocalInner>,
    rd_event: Event,
    wr_event: Event,
    accept_event: Event,
}

fn current_cred() -> ucred {
    let proc = Scheduler::get_current().get_process();
    let identity = proc.identity.lock();
    ucred {
        pid: proc.get_pid(),
        uid: identity.effective_user_id,
        gid: identity.effective_group_id,
    }
}

fn make_socket(state: State, sock_type: u32) -> EResult<Arc<LocalSocket>> {
    let this = Arc::try_new(LocalSocket {
        inner: SpinMutex::new(LocalInner {
            state,
            sock_type,
            bound_addr: None,
            peer: None,
            self_ref: Weak::new(),
            recv_buf: RingBuffer::new(BUFFER_SIZE),
            inflight: VecDeque::new(),
            backlog: VecDeque::new(),
            backlog_limit: 0,
            shutdown: ShutdownFlags::empty(),
            peer_closed: false,
            owner_cred: current_cred(),
        }),
        rd_event: Event::new(),
        wr_event: Event::new(),
        accept_event: Event::new(),
    })?;
    this.inner.lock().self_ref = Arc::downgrade(&this);
    Ok(this)
}

impl LocalSocket {
    pub fn new(sock_type: u32) -> EResult<Arc<Self>> {
        make_socket(State::Unconnected, sock_type)
    }

    pub fn new_pair(sock_type: u32) -> EResult<(Arc<Socket>, Arc<Socket>)> {
        let a = make_socket(State::Connected, sock_type)?;
        let b = make_socket(State::Connected, sock_type)?;

        a.inner.lock().peer = Some(b.clone());
        b.inner.lock().peer = Some(a.clone());

        let sa = Socket::new(AF_UNIX, sock_type, a)?;
        let sb = Socket::new(AF_UNIX, sock_type, b)?;
        Ok((sa, sb))
    }

    fn parse_path(addr: &[u8]) -> EResult<&[u8]> {
        if addr.len() < size_of::<sa_family_t>() + 1 {
            return Err(Errno::EINVAL);
        }
        let path_bytes = &addr[size_of::<sa_family_t>()..];
        let path_len = path_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_bytes.len());
        if path_len == 0 {
            return Err(Errno::EINVAL);
        }
        Ok(&path_bytes[..path_len])
    }

    fn build_sockaddr(path: &[u8]) -> Vec<u8> {
        let family_size = size_of::<sa_family_t>();
        let mut addr = vec![0u8; family_size + path.len() + 1];
        let family = AF_UNIX as sa_family_t;
        addr[..family_size].copy_from_slice(&family.to_ne_bytes());
        addr[family_size..family_size + path.len()].copy_from_slice(path);
        addr
    }

    fn maybe_sigpipe(&self, flags: u32) {
        if flags & MSG_NOSIGNAL != 0 {
            return;
        }
        send_signal_to_thread(&Scheduler::get_current(), Signal::SigPipe);
    }

    /// Walk a user-layout cmsg blob, collecting SCM_RIGHTS fds as owned `Arc<File>`.
    /// Unknown cmsgs are ignored (matches glibc/Astral behaviour).
    fn parse_scm_rights(
        control: &[u8],
        files: &mut Vec<Arc<File>>,
        has_rights: &mut bool,
    ) -> EResult<()> {
        let hdr_size = size_of::<cmsghdr>();
        let mut off = 0usize;
        let proc = Scheduler::get_current().get_process();
        let fdtable = proc.open_files.lock();

        while off + hdr_size <= control.len() {
            let mut hdr = cmsghdr {
                cmsg_len: 0,
                cmsg_level: 0,
                cmsg_type: 0,
            };
            // Safety: cmsghdr is #[repr(C)] Copy of plain scalars.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    control.as_ptr().add(off),
                    &mut hdr as *mut _ as *mut u8,
                    hdr_size,
                );
            }

            let cmsg_len = hdr.cmsg_len as usize;
            if cmsg_len < hdr_size || off + cmsg_len > control.len() {
                break;
            }

            if hdr.cmsg_level == SOL_SOCKET as i32 && hdr.cmsg_type == SCM_RIGHTS as i32 {
                *has_rights = true;
                let data_off = off + cmsg_align(hdr_size);
                let payload_len = cmsg_len - cmsg_align(hdr_size);
                let fd_count = payload_len / size_of::<i32>();
                for i in 0..fd_count {
                    let mut fd_bytes = [0u8; 4];
                    fd_bytes.copy_from_slice(&control[data_off + i * 4..data_off + i * 4 + 4]);
                    let fd = i32::from_ne_bytes(fd_bytes);
                    let desc = fdtable.get_fd(fd).ok_or(Errno::EBADF)?;
                    files.push(desc.file);
                }
            }

            // Advance past this cmsg, padded to alignof(size_t).
            off += cmsg_align(cmsg_len);
        }

        Ok(())
    }

    fn build_cmsg(
        inflight: &mut VecDeque<InflightSection>,
        control: &mut [u8],
        flags: u32,
        out_flags: &mut u32,
    ) -> EResult<usize> {
        // A caller that didn't ask for ancillary data (plain read / recv)
        // leaves fds pending for a future recvmsg rather than dropping them.
        if control.is_empty() {
            return Ok(0);
        }

        let Some(section) = inflight.front_mut() else {
            return Ok(0);
        };
        if section.files.is_empty() {
            return Ok(0);
        }

        let hdr_size = size_of::<cmsghdr>();
        let header_aligned = cmsg_align(hdr_size);

        let available = control.len();
        if available <= header_aligned {
            // Userspace gave us a buffer too small even for one fd — drop
            // them all and signal truncation, matching Linux behaviour.
            let _ = core::mem::take(&mut section.files);
            *out_flags |= MSG_CTRUNC;
            return Ok(0);
        }

        let fd_slot_count = (available - header_aligned) / size_of::<i32>();
        let n = min(section.files.len(), fd_slot_count);

        let cloexec = flags & MSG_CMSG_CLOEXEC != 0;

        let proc = Scheduler::get_current().get_process();
        let mut installed_fds: Vec<i32> = Vec::with_capacity(n);

        let taken: Vec<Arc<File>> = section.files.drain(..n).collect();
        {
            let mut fdtable = proc.open_files.lock();
            for file in taken {
                let desc = FileDescription {
                    file,
                    close_on_exec: AtomicBool::new(cloexec),
                };
                match fdtable.open_file(desc, 0) {
                    Some(fd) => installed_fds.push(fd),
                    None => {
                        // Out of fds: stop here, truncate the remainder.
                        *out_flags |= MSG_CTRUNC;
                        break;
                    }
                }
            }
        }

        // Discard any fds in this section that we could not (or chose not to) deliver.
        // Dropping the Arc<File> runs File::close when the last ref goes away.
        if !section.files.is_empty() {
            *out_flags |= MSG_CTRUNC;
            section.files.clear();
        }

        let installed = installed_fds.len();
        let payload = installed * size_of::<i32>();
        let total_len = header_aligned + payload;

        let hdr = cmsghdr {
            cmsg_len: cmsg_len(payload) as socklen_t,
            cmsg_level: SOL_SOCKET as i32,
            cmsg_type: SCM_RIGHTS as i32,
        };

        // Write header.
        unsafe {
            core::ptr::copy_nonoverlapping(
                &hdr as *const _ as *const u8,
                control.as_mut_ptr(),
                hdr_size,
            );
        }
        // Zero any header padding up to the data offset.
        for b in &mut control[hdr_size..header_aligned] {
            *b = 0;
        }
        // Write fds.
        for (i, fd) in installed_fds.iter().enumerate() {
            let off = header_aligned + i * size_of::<i32>();
            control[off..off + 4].copy_from_slice(&fd.to_ne_bytes());
        }

        Ok(total_len)
    }

    fn write_addr(addr: &Option<Vec<u8>>, buf: &mut [u8]) -> usize {
        match addr {
            Some(a) => {
                let len = min(a.len(), buf.len());
                buf[..len].copy_from_slice(&a[..len]);
                a.len()
            }
            None => {
                let family = AF_UNIX as sa_family_t;
                let bytes = family.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                size_of::<sa_family_t>()
            }
        }
    }
}

impl SocketOps for LocalSocket {
    fn bind(&self, addr: &[u8], socket: &Arc<Socket>) -> EResult<()> {
        let path = Self::parse_path(addr)?;

        {
            let mut inner = self.inner.lock();
            if inner.state != State::Unconnected {
                return Err(Errno::EINVAL);
            }
            inner.bound_addr = Some(Self::build_sockaddr(path));
            inner.state = State::Bound;
        }

        let proc = Scheduler::get_current().get_process();
        let root = proc.root_dir.lock().clone();
        let cwd = proc.working_dir.lock().clone();
        let identity = proc.identity.lock().clone();
        let mode = Mode::from_bits_truncate(0o777);
        vfs::mknod(
            root,
            cwd,
            path,
            mode,
            Some(Device::Socket(socket.clone())),
            &identity,
        )?;

        Ok(())
    }

    fn listen(&self, backlog: i32) -> EResult<()> {
        let mut inner = self.inner.lock();
        match inner.state {
            State::Bound | State::Listening => {
                inner.state = State::Listening;
                inner.backlog_limit = (backlog as usize).max(1);
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn accept(&self, nonblocking: bool) -> EResult<Arc<Socket>> {
        let guard = self.accept_event.guard();
        loop {
            {
                let mut inner = self.inner.lock();
                if inner.state != State::Listening {
                    return Err(Errno::EINVAL);
                }
                if let Some(sock) = inner.backlog.pop_front() {
                    return Ok(sock);
                }
            }
            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            guard.wait();
            if crate::sched::Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn connect(&self, addr: &[u8], _nonblocking: bool) -> EResult<()> {
        let path = Self::parse_path(addr)?;

        let (sock_type, self_arc) = {
            let inner = self.inner.lock();
            if inner.state == State::Connected {
                return Err(Errno::EISCONN);
            }
            if inner.state == State::Listening {
                return Err(Errno::EINVAL);
            }
            let self_arc = inner.self_ref.upgrade().ok_or(Errno::EBADF)?;
            (inner.sock_type, self_arc)
        };

        let proc = Scheduler::get_current().get_process();
        let root = proc.root_dir.lock().clone();
        let cwd = proc.working_dir.lock().clone();
        let identity = proc.identity.lock().clone();

        let target = PathNode::lookup(
            root,
            cwd,
            path,
            &identity,
            LookupFlags::MustExist | LookupFlags::FollowSymlinks,
        )?;

        let inode = target.entry.get_inode().ok_or(Errno::ENOENT)?;
        let listener_socket = match &inode.node_ops {
            NodeOps::Socket(s) => s.clone(),
            _ => return Err(Errno::ECONNREFUSED),
        };

        let listener: Arc<LocalSocket> =
            Arc::downcast(listener_socket.ops.clone()).map_err(|_| Errno::ECONNREFUSED)?;

        let server_end = make_socket(State::Connected, sock_type)?;

        {
            let mut listener_inner = listener.inner.lock();
            if listener_inner.state != State::Listening {
                return Err(Errno::ECONNREFUSED);
            }
            if listener_inner.backlog.len() >= listener_inner.backlog_limit {
                return Err(Errno::ECONNREFUSED);
            }

            let listener_cred = listener_inner.owner_cred;
            {
                let mut server_inner = server_end.inner.lock();
                server_inner.peer = Some(self_arc);
                server_inner.owner_cred = listener_cred;
            }

            let server_socket = Socket::new(AF_UNIX, sock_type, server_end.clone())?;
            listener_inner.backlog.push_back(server_socket);
        }

        {
            let mut self_inner = self.inner.lock();
            self_inner.peer = Some(server_end);
            self_inner.state = State::Connected;
        }

        listener.accept_event.wake_one();
        listener.rd_event.wake_one();

        Ok(())
    }

    fn send(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize> {
        self.sendmsg(buf, &[], flags, nonblocking)
    }

    fn recv(&self, buf: &mut IovecIter, flags: u32, nonblocking: bool) -> EResult<isize> {
        let (n, _, _) = self.recvmsg(buf, &mut [], flags, nonblocking)?;
        Ok(n)
    }

    fn sendmsg(
        &self,
        buf: &mut IovecIter,
        control: &[u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<isize> {
        if buf.is_empty() && control.is_empty() {
            return Ok(0);
        }

        let peer = {
            let inner = self.inner.lock();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            if inner.shutdown.contains(ShutdownFlags::Write) {
                self.maybe_sigpipe(flags);
                return Err(Errno::EPIPE);
            }
            inner.peer.clone().ok_or(Errno::ENOTCONN)?
        };

        // Resolve any SCM_RIGHTS fds up-front so we can fail cleanly without
        // having partially committed data to the peer's recv buffer.
        let mut files_to_send: Vec<Arc<File>> = Vec::new();
        let mut has_rights_cmsg = false;
        Self::parse_scm_rights(control, &mut files_to_send, &mut has_rights_cmsg)?;

        // Zero-length send: still deliver any fds to the peer's inflight
        // queue (attached to the next data that arrives) and return.
        if buf.is_empty() {
            let mut peer_inner = peer.inner.lock();
            if peer_inner.peer_closed || peer_inner.shutdown.contains(ShutdownFlags::Read) {
                self.maybe_sigpipe(flags);
                return Err(Errno::EPIPE);
            }
            if has_rights_cmsg {
                push_inflight(&mut peer_inner.inflight, &mut files_to_send, 0);
            }
            return Ok(0);
        }

        let wr_guard = self.wr_event.guard();
        loop {
            {
                let mut peer_inner = peer.inner.lock();
                if peer_inner.peer_closed || peer_inner.shutdown.contains(ShutdownFlags::Read) {
                    self.maybe_sigpipe(flags);
                    return Err(Errno::EPIPE);
                }

                let available = peer_inner.recv_buf.get_available_len();
                if available > 0 {
                    let want = min(buf.len() - buf.total_offset(), available);
                    let mut scratch = vec![0u8; min(want, BUFFER_SIZE)];
                    buf.copy_to_slice(&mut scratch)?;
                    let written = peer_inner.recv_buf.write(&scratch);

                    if written > 0 {
                        push_inflight(&mut peer_inner.inflight, &mut files_to_send, written);
                        peer.rd_event.wake_one();
                        return Ok(written as isize);
                    }
                }
            }

            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            wr_guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn recvmsg(
        &self,
        buf: &mut IovecIter,
        control: &mut [u8],
        flags: u32,
        nonblocking: bool,
    ) -> EResult<(isize, usize, u32)> {
        if buf.is_empty() && control.is_empty() {
            return Ok((0, 0, 0));
        }

        let peek = flags & MSG_PEEK != 0;

        let rd_guard = self.rd_event.guard();
        loop {
            {
                let mut inner = self.inner.lock();
                if inner.state != State::Connected {
                    return Err(Errno::ENOTCONN);
                }

                let data_len = inner.recv_buf.get_data_len();

                if data_len > 0 {
                    let mut recvcount = min(buf.len(), data_len);
                    // Clamp to the current barrier section so we don't read
                    // past data that belongs to a later fd-carrying message.
                    if let Some(front) = inner.inflight.front()
                        && front.bytes > 0
                    {
                        recvcount = min(recvcount, front.bytes);
                    }

                    let mut scratch = vec![0u8; min(recvcount, BUFFER_SIZE)];
                    let len = inner.recv_buf.peek(&mut scratch);

                    if len > 0 {
                        buf.copy_from_slice(&scratch[..len])?;
                    }

                    // Data made it to userspace (or there was none). Now
                    // commit: install cmsg fds and advance the ring cursor.
                    let mut out_flags = 0u32;
                    let ctrl_written =
                        Self::build_cmsg(&mut inner.inflight, control, flags, &mut out_flags)?;

                    if len > 0 && !peek {
                        let mut drop = vec![0u8; len];
                        inner.recv_buf.read(&mut drop);

                        if let Some(front) = inner.inflight.front_mut() {
                            front.bytes = front.bytes.saturating_sub(len);
                            // Advance to the next section once this one's data range is drained.
                            // Any fds still sitting here were not delivered (caller didn't pass a control buffer).
                            if front.bytes == 0 && inner.inflight.len() > 1 {
                                inner.inflight.pop_front();
                            }
                        }
                        self.wr_event.wake_one();
                    }

                    return Ok((len as isize, ctrl_written, out_flags));
                }

                if inner.shutdown.contains(ShutdownFlags::Read) || inner.peer_closed {
                    // Even on EOF, hand back any remaining inflight fds.
                    let mut out_flags = 0u32;
                    let ctrl_written =
                        Self::build_cmsg(&mut inner.inflight, control, flags, &mut out_flags)?;
                    return Ok((0, ctrl_written, out_flags));
                }
            }

            if nonblocking {
                return Err(Errno::EAGAIN);
            }
            rd_guard.wait();
            if Scheduler::get_current().has_pending_signals() {
                return Err(Errno::EINTR);
            }
        }
    }

    fn shutdown(&self, how: u32) -> EResult<()> {
        let flags = ShutdownFlags::from_bits_truncate(how);
        let peer = {
            let mut inner = self.inner.lock();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            inner.shutdown |= flags;
            inner.peer.clone()
        };

        if flags.contains(ShutdownFlags::Read) {
            self.rd_event.wake_all();
        }
        if flags.contains(ShutdownFlags::Write) {
            if let Some(peer) = &peer {
                peer.rd_event.wake_all();
            }
            self.wr_event.wake_all();
        }
        Ok(())
    }

    fn getsockname(&self, buf: &mut [u8]) -> EResult<usize> {
        let inner = self.inner.lock();
        Ok(Self::write_addr(&inner.bound_addr, buf))
    }

    fn getpeername(&self, buf: &mut [u8]) -> EResult<usize> {
        let inner = self.inner.lock();
        if inner.state != State::Connected {
            return Err(Errno::ENOTCONN);
        }
        let peer_addr = inner
            .peer
            .as_ref()
            .and_then(|p| p.inner.lock().bound_addr.clone());
        Ok(Self::write_addr(&peer_addr, buf))
    }

    fn getsockopt(&self, level: i32, optname: i32, buf: &mut [u8]) -> EResult<usize> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        match optname as u32 {
            SO_TYPE => {
                let val = self.inner.lock().sock_type as i32;
                let bytes = val.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_ERROR => {
                let bytes = 0i32.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_SNDBUF | SO_RCVBUF => {
                let val = BUFFER_SIZE as i32;
                let bytes = val.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_ACCEPTCONN => {
                let val = (self.inner.lock().state == State::Listening) as i32;
                let bytes = val.to_ne_bytes();
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<i32>())
            }
            SO_PEERCRED => {
                let peer = self.inner.lock().peer.clone().ok_or(Errno::ENOTCONN)?;
                let cred = peer.inner.lock().owner_cred;
                let mut bytes = [0u8; size_of::<ucred>()];
                bytes[0..4].copy_from_slice(&cred.pid.to_ne_bytes());
                bytes[4..8].copy_from_slice(&cred.uid.to_ne_bytes());
                bytes[8..12].copy_from_slice(&cred.gid.to_ne_bytes());
                let len = min(bytes.len(), buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(size_of::<ucred>())
            }
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    fn setsockopt(&self, level: i32, optname: i32, _buf: &[u8]) -> EResult<()> {
        if level as u32 != SOL_SOCKET {
            return Err(Errno::ENOPROTOOPT);
        }
        match optname as u32 {
            SO_SNDBUF | SO_RCVBUF | SO_PASSCRED | SO_REUSEADDR => Ok(()),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    fn poll(&self, mask: PollFlags) -> EResult<PollFlags> {
        let inner = self.inner.lock();
        let mut revents = PollFlags::empty();

        match inner.state {
            State::Listening => {
                if !inner.backlog.is_empty() {
                    revents |= PollFlags::In;
                }
            }
            State::Connected => {
                if inner.recv_buf.get_data_len() > 0 || inner.peer_closed {
                    revents |= PollFlags::In;
                }
                if inner.shutdown.contains(ShutdownFlags::Read) {
                    revents |= PollFlags::Hup;
                }
                if inner.peer_closed {
                    revents |= PollFlags::Hup;
                }
                let peer_can_recv = inner
                    .peer
                    .as_ref()
                    .is_some_and(|p| p.inner.lock().recv_buf.get_available_len() > 0);
                if peer_can_recv && !inner.shutdown.contains(ShutdownFlags::Write) {
                    revents |= PollFlags::Out;
                }
                if inner.shutdown.contains(ShutdownFlags::Write) && inner.peer_closed {
                    revents |= PollFlags::Err;
                }
            }
            _ => {
                revents |= PollFlags::Out;
            }
        }

        Ok(revents & (mask | PollFlags::Err | PollFlags::Hup))
    }

    fn poll_events(&self, mask: PollFlags) -> PollEventSet<'_> {
        let wants_read = mask.intersects(PollFlags::Read);
        let wants_write = mask.intersects(PollFlags::Write);

        let mut events = PollEventSet::new();
        if wants_read || !wants_write {
            events = events.add(&self.rd_event);
        }
        if wants_write || !wants_read {
            events = events.add(&self.wr_event);
        }
        events
    }

    fn release(&self) -> EResult<()> {
        let (peer, inflight) = {
            let mut inner = self.inner.lock();
            inner.state = State::Unconnected;
            let peer = inner.peer.take();
            // Drain any in-flight sections.
            let inflight: Vec<InflightSection> = inner.inflight.drain(..).collect();
            inner.backlog.clear();
            (peer, inflight)
        };

        // Close any undelivered in-flight files. If we hold the last Arc, File::close runs the underlying ops.release.
        for section in inflight {
            for file in section.files {
                if Arc::strong_count(&file) == 1 {
                    let _ = file.close();
                }
            }
        }

        if let Some(peer) = peer {
            peer.inner.lock().peer_closed = true;
            peer.rd_event.wake_all();
            peer.wr_event.wake_all();
        }

        Ok(())
    }
}
