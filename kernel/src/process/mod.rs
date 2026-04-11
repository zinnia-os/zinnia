pub mod signal;
pub mod task;

use crate::{
    INIT,
    arch::sched::Context,
    device::tty::Tty,
    memory::{VirtAddr, virt::AddressSpace},
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::{signal::SignalState, task::Task},
    sched::Scheduler,
    uapi,
    util::{event::Event, mutex::spin::SpinMutex, once::Once},
    vfs::{
        self,
        cache::PathNode,
        exec::ExecInfo,
        file::{File, FileDescription},
    },
};
use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicUsize, Ordering};

/// A unique process ID.
pub type Pid = usize;

#[derive(Debug)]
pub enum ProcessState {
    Running,
    Exited(u8),
    // TODO: SIGSTOP
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IntervalTimerState {
    interval_ns: usize,
    next_deadline_ns: Option<usize>,
}

impl IntervalTimerState {
    fn snapshot(&self, now: usize) -> uapi::time::itimerval {
        uapi::time::itimerval {
            it_interval: ns_to_timeval(self.interval_ns),
            it_value: ns_to_timeval(
                self.next_deadline_ns
                    .map(|deadline| deadline.saturating_sub(now))
                    .unwrap_or(0),
            ),
        }
    }

    fn replace(
        &mut self,
        now: usize,
        value: uapi::time::itimerval,
    ) -> EResult<uapi::time::itimerval> {
        let old = self.snapshot(now);
        self.interval_ns = timeval_to_ns(value.it_interval)?;

        let initial_ns = timeval_to_ns(value.it_value)?;
        self.next_deadline_ns = if initial_ns == 0 {
            None
        } else {
            Some(now.checked_add(initial_ns).ok_or(Errno::EINVAL)?)
        };

        Ok(old)
    }
}

pub struct Process {
    /// The unique identifier of this process.
    id: Pid,
    /// The display name of this process.
    name: String,
    /// The parent of this process, or [`None`], if this is the init process.
    parent: Option<Weak<Process>>,
    /// A list of [`Task`]s associated with this process.
    pub threads: SpinMutex<Vec<Arc<Task>>>,
    /// The address space for this process.
    pub address_space: Arc<SpinMutex<AddressSpace>>,
    /// The root directory for this process.
    pub root_dir: SpinMutex<PathNode>,
    /// Current working directory.
    pub working_dir: SpinMutex<PathNode>,
    /// The status of this process.
    pub status: SpinMutex<ProcessState>,
    /// Child processes owned by this process.
    pub children: SpinMutex<Vec<Arc<Process>>>,
    /// The user identity of this process.
    pub identity: SpinMutex<Identity>,
    /// A table of open file descriptors.
    pub open_files: SpinMutex<FdTable>,
    /// Per-process signal action table.
    pub signal_actions: SpinMutex<SignalState>,
    /// A pointer to the next free memory region.
    pub mmap_head: SpinMutex<VirtAddr>,
    /// Process group ID.
    pub pgrp: SpinMutex<Pid>,
    /// Session ID.
    pub session: SpinMutex<Pid>,
    /// Controlling terminal, if any.
    pub controlling_tty: SpinMutex<Option<Arc<Tty>>>,
    /// Process-wide real-time interval timer.
    pub real_timer: SpinMutex<IntervalTimerState>,
    /// Event that is signalled when a child process exits.
    pub child_event: Event,
}

impl Process {
    /// Returns the unique identifier of this process.
    #[inline]
    pub const fn get_pid(&self) -> Pid {
        self.id
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_real_timer(&self, now: usize) -> uapi::time::itimerval {
        self.real_timer.lock().snapshot(now)
    }

    pub fn set_real_timer(
        &self,
        now: usize,
        value: uapi::time::itimerval,
    ) -> EResult<uapi::time::itimerval> {
        self.real_timer.lock().replace(now, value)
    }

    /// Gets the parent process of this process.
    /// Returns [`None`], if it is the init process.
    pub fn get_parent(&self) -> Option<Arc<Self>> {
        // TODO: The upgrade should never fail.
        // If it does, then somehow the child was alive but the parent was not.
        self.parent.as_ref().map(|x| {
            x.upgrade()
                .expect("FIXME: Child process was alive for longer than the parent")
        })
    }

    pub fn new(name: String, parent: Option<Arc<Self>>) -> EResult<Self> {
        Self::new_with_space(name, parent, AddressSpace::new())
    }

    pub fn fork(self: Arc<Self>, context: &Context) -> EResult<(Arc<Self>, Arc<Task>)> {
        let forked = Arc::new(Self {
            id: PID_COUNTER.fetch_add(1, Ordering::Acquire),
            name: self.name.clone(),
            parent: Some(Arc::downgrade(&self)),
            threads: SpinMutex::new(Vec::new()),
            address_space: Arc::new(SpinMutex::new(self.address_space.lock().fork()?)),
            root_dir: SpinMutex::new(self.root_dir.lock().clone()),
            working_dir: SpinMutex::new(self.working_dir.lock().clone()),
            status: SpinMutex::new(ProcessState::Running),
            children: SpinMutex::new(Vec::new()),
            identity: SpinMutex::new(self.identity.lock().clone()),
            open_files: SpinMutex::new(self.open_files.lock().clone()),
            signal_actions: SpinMutex::new(self.signal_actions.lock().clone()),
            mmap_head: SpinMutex::new(*self.mmap_head.lock()),
            pgrp: SpinMutex::new(*self.pgrp.lock()),
            session: SpinMutex::new(*self.session.lock()),
            controlling_tty: SpinMutex::new(self.controlling_tty.lock().clone()),
            real_timer: SpinMutex::new(*self.real_timer.lock()),
            child_event: Event::new(),
        });

        // Create a heap allocated context that we can pass to the entry point.
        let mut forked_ctx = Box::new(*context);
        forked_ctx.set_return(0, 0); // User mode returns 0 for forked processes.
        let raw_ctx = Box::into_raw(forked_ctx);

        // Create the main thread.
        let forked_thread = Arc::new(Task::new(to_user_context, raw_ctx as _, 0, &forked, true)?);
        forked.threads.lock().push(forked_thread.clone());
        self.children.lock().push(forked.clone());
        PROCESS_TABLE
            .lock()
            .insert(forked.get_pid(), Arc::downgrade(&forked));

        Ok((forked, forked_thread))
    }

    fn new_with_space(
        name: String,
        parent: Option<Arc<Self>>,
        space: AddressSpace,
    ) -> EResult<Self> {
        let (root, cwd, identity, pgrp, session, ctty) = match &parent {
            Some(x) => (
                x.root_dir.lock().clone(),
                x.working_dir.lock().clone(),
                x.identity.lock().clone(),
                *x.pgrp.lock(),
                *x.session.lock(),
                x.controlling_tty.lock().clone(),
            ),
            None => (
                vfs::get_root(),
                vfs::get_root(),
                Identity::default(),
                0,
                0,
                None,
            ),
        };

        // NOTE: The child is not yet an Arc here, so we cannot add it to the
        // parent's children list. Callers that wrap this in Arc (e.g. fork)
        // are responsible for registering the child.

        let id = PID_COUNTER.fetch_add(1, Ordering::Relaxed);
        // For the very first processes (kernel, init) pgrp/session default to their own PID.
        let pgrp = if pgrp == 0 { id } else { pgrp };
        let session = if session == 0 { id } else { session };

        Ok(Self {
            id,
            name,
            parent: parent.map(|x| Arc::downgrade(&x)),
            threads: SpinMutex::new(Vec::new()),
            address_space: Arc::new(SpinMutex::new(space)),
            status: SpinMutex::new(ProcessState::Running),
            children: SpinMutex::new(Vec::new()),
            root_dir: SpinMutex::new(root),
            working_dir: SpinMutex::new(cwd),
            identity: SpinMutex::new(identity),
            open_files: SpinMutex::new(FdTable::new()),
            signal_actions: SpinMutex::new(SignalState::new()),
            // TODO: This address should be determined from the highest loaded segment.
            mmap_head: SpinMutex::new(VirtAddr::new(0x1_0000_0000)),
            pgrp: SpinMutex::new(pgrp),
            session: SpinMutex::new(session),
            controlling_tty: SpinMutex::new(ctty),
            real_timer: SpinMutex::new(IntervalTimerState::default()),
            child_event: Event::new(),
        })
    }

    /// Returns the kernel process.
    pub fn get_kernel() -> &'static Arc<Self> {
        KERNEL_PROCESS.get()
    }

    /// Replaces a process with a new executable image, given some arguments and an environment.
    /// The given file must be opened with ReadOnly and Executable.
    /// Any existing threads of the current process are destroyed upon a successful execve.
    /// This also means that a successful execve will never return.
    pub fn fexecve(
        self: Arc<Self>,
        file: Arc<File>,
        argv: Vec<Vec<u8>>,
        envp: Vec<Vec<u8>>,
    ) -> EResult<()> {
        let mut info = ExecInfo {
            executable: file.clone(),
            interpreter: None,
            space: AddressSpace::new(),
            argv,
            envp,
        };

        let format = vfs::exec::identify(&file).ok_or(Errno::ENOEXEC)?;
        let init = Arc::try_new(format.load(&self, &mut info)?)?;

        // If we get here, then the loading of the executable was successful.
        {
            let mut threads = self.threads.lock();
            threads.clear();
            threads.push(init.clone());

            let mut space = self.address_space.lock();
            *space = info.space;

            self.open_files.lock().close_exec();
            self.signal_actions.lock().reset_on_exec();
        }

        CpuData::get().scheduler.add_task(init);

        // execve never returns on success.
        Scheduler::kill_current();
    }

    /// Exits the current process.
    pub fn exit(code: u8) -> ! {
        let task = Scheduler::get_current();
        let proc = task.get_process();

        if proc.get_pid() <= 1 {
            panic!("Attempted to kill init with error code {code}");
        }

        PROCESS_TABLE.lock().remove(&proc.get_pid());
        {
            let mut open_files = proc.open_files.lock();
            let mut threads = proc.threads.lock();
            let mut status = proc.status.lock();

            // Kill all threads.
            for thread in threads.iter() {
                *thread.state.lock() = task::TaskState::Dead;
            }
            threads.clear();

            // Close all files.
            open_files.close_all();

            *status = ProcessState::Exited(code);
        }

        // Reparent orphaned children to the init process.
        {
            let mut our_children = proc.children.lock();
            if !our_children.is_empty() {
                let init = INIT.get();
                let mut init_children = init.children.lock();
                for child in our_children.drain(..) {
                    init_children.push(child);
                }
                // Wake init in case any of these children are already exited.
                init.child_event.wake_all();
            }
        }

        // Wake the parent so that waitpid can collect this child.
        if let Some(parent) = proc.get_parent() {
            parent.child_event.wake_all();
        }

        Scheduler::kill_current();
    }
}

#[repr(transparent)]
#[derive(Clone)]
pub struct FdTable {
    inner: BTreeMap<i32, FileDescription>,
}

impl FdTable {
    pub const fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// Attempts to get the file corresponding to the given file descriptor.
    /// Note that this does not handle special FDs like [`uapi::fcntl::AT_FDCWD`].
    pub fn get_fd(&self, fd: i32) -> Option<FileDescription> {
        // Negative FDs are never valid.
        if fd.is_negative() {
            return None;
        }

        self.inner.get(&fd).cloned()
    }

    /// Allocates a new descriptor for a file. Returns [`None`] if there are no more free FDs for this process.
    pub fn open_file(&mut self, file: FileDescription, base: i32) -> Option<i32> {
        // TODO: OPEN_MAX
        // Find a free descriptor.
        let mut last = base;
        loop {
            if !self.inner.contains_key(&last) {
                break;
            }
            last += 1;
        }

        self.inner.insert(last, file);
        Some(last)
    }

    pub fn close(&mut self, fd: i32) -> Option<()> {
        let desc = self.inner.remove(&fd);
        match desc {
            Some(desc) => {
                if Arc::strong_count(&desc.file) == 1 {
                    _ = desc.file.close();
                }
                Some(())
            }
            None => None,
        }
    }

    pub fn close_all(&mut self) {
        let fds = self.inner.keys().cloned().collect::<Vec<_>>();
        for fd in fds {
            let desc = self.inner.remove(&fd);
            if let Some(desc) = desc
                && Arc::strong_count(&desc.file) == 1
            {
                _ = desc.file.close();
            }
        }
        self.inner.clear();
    }

    /// Closes all files with the [`OpenFlags::CloseOnExec`] flag.
    pub fn close_exec(&mut self) {
        let fds = self.inner.keys().cloned().collect::<Vec<_>>();
        for fd in fds {
            if !self
                .inner
                .get(&fd)
                .unwrap()
                .close_on_exec
                .load(Ordering::Acquire)
            {
                continue;
            }

            let desc = self.inner.remove(&fd);
            if let Some(desc) = desc
                && Arc::strong_count(&desc.file) == 1
            {
                _ = desc.file.close();
            }
        }
    }
}

/// Entry point for tasks wanting to jump to user space.
pub extern "C" fn to_user(ip: usize, sp: usize) {
    unsafe { crate::arch::sched::jump_to_user(VirtAddr::from(ip), VirtAddr::from(sp)) };
}

/// Entry point for tasks wanting to jump to user space.
pub extern "C" fn to_user_context(context: usize, _: usize) {
    unsafe {
        let ctx: Box<Context> = Box::from_raw(context as _);
        let mut stack_ctx = Box::into_inner(ctx);
        crate::arch::sched::jump_to_context(&raw mut stack_ctx)
    };
}

#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub user_id: uapi::uid_t,
    pub group_id: uapi::gid_t,

    pub effective_user_id: uapi::uid_t,
    pub effective_group_id: uapi::gid_t,

    pub set_user_id: uapi::uid_t,
    pub set_group_id: uapi::gid_t,
}

impl Identity {
    /// Returns an identity suitable for kernel accesses, with absolute privileges for everything.
    pub fn get_kernel() -> &'static Identity {
        static KERNEL_IDENTITY: Identity = Identity {
            user_id: 0,
            group_id: 0,
            effective_user_id: 0,
            effective_group_id: 0,
            set_user_id: 0,
            set_group_id: 0,
        };
        &KERNEL_IDENTITY
    }

    pub const fn is_superuser(&self) -> bool {
        self.user_id == 0
    }

    pub const fn is_effective_superuser(&self) -> bool {
        self.effective_user_id == 0
    }

    pub const fn is_set_superuser(&self) -> bool {
        self.set_user_id == 0
    }
}

static PID_COUNTER: AtomicUsize = AtomicUsize::new(0);
static KERNEL_PROCESS: Once<Arc<Process>> = Once::new();

/// Global table of all live processes, keyed by PID.
/// Used to iterate processes for signal delivery to process groups.
pub static PROCESS_TABLE: SpinMutex<BTreeMap<Pid, Weak<Process>>> = SpinMutex::new(BTreeMap::new());

pub fn poll_interval_timers(now: usize) {
    let processes: Vec<_> = {
        let table = PROCESS_TABLE.lock();
        table.values().filter_map(Weak::upgrade).collect()
    };

    for proc in processes {
        let should_signal = {
            let mut timer = proc.real_timer.lock();
            match timer.next_deadline_ns {
                Some(deadline) if deadline <= now => {
                    if timer.interval_ns == 0 {
                        timer.next_deadline_ns = None;
                    } else {
                        let mut next_deadline = deadline;
                        loop {
                            let Some(candidate) = next_deadline.checked_add(timer.interval_ns)
                            else {
                                timer.next_deadline_ns = None;
                                break;
                            };

                            if candidate > now {
                                timer.next_deadline_ns = Some(candidate);
                                break;
                            }

                            next_deadline = candidate;
                        }
                    }

                    true
                }
                Some(_) | None => false,
            }
        };

        if !should_signal {
            continue;
        }

        let thread = {
            let threads = proc.threads.lock();
            threads.first().cloned()
        };

        if let Some(thread) = thread {
            signal::send_signal_to_thread(&thread, signal::Signal::SIGALRM);
        }
    }
}

fn timeval_to_ns(value: uapi::time::timeval) -> EResult<usize> {
    if value.tv_sec < 0 || value.tv_usec < 0 || value.tv_usec >= 1_000_000 {
        return Err(Errno::EINVAL);
    }

    let seconds = (value.tv_sec as usize)
        .checked_mul(1_000_000_000)
        .ok_or(Errno::EINVAL)?;
    let micros = (value.tv_usec as usize)
        .checked_mul(1_000)
        .ok_or(Errno::EINVAL)?;

    seconds.checked_add(micros).ok_or(Errno::EINVAL)
}

fn ns_to_timeval(value: usize) -> uapi::time::timeval {
    uapi::time::timeval {
        tv_sec: (value / 1_000_000_000) as _,
        tv_usec: ((value % 1_000_000_000) / 1_000) as _,
    }
}

#[initgraph::task(
    name = "generic.process",
    depends = [crate::memory::MEMORY_STAGE, crate::vfs::VFS_STAGE],
)]
pub fn PROCESS_STAGE() {
    // Create the kernel process and task.
    unsafe {
        KERNEL_PROCESS.init(Arc::new(
            Process::new_with_space(
                "kernel".into(),
                None,
                AddressSpace {
                    table: super::memory::virt::KERNEL_PAGE_TABLE.get().clone(),
                    mappings: BTreeSet::new(),
                },
            )
            .expect("Unable to create the main kernel process"),
        ))
    };

    let kproc = KERNEL_PROCESS.get();
    PROCESS_TABLE
        .lock()
        .insert(kproc.get_pid(), Arc::downgrade(&kproc));
}
