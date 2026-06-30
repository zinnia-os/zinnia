pub mod itimer;
pub mod signal;
pub mod task;

use crate::{
    INIT,
    arch::sched::Context,
    device::tty::Tty,
    memory::{
        VirtAddr,
        virt::{AddressSpace, KERNEL_PAGE_TABLE},
    },
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::{
        itimer::IntervalTimerState,
        signal::{Signal, SignalState},
        task::Task,
    },
    sched::Scheduler,
    uapi,
    util::{
        event::Event,
        mutex::{Mutex, spin::SpinMutex},
        once::Once,
    },
    vfs::{
        self,
        cache::PathNode,
        exec::ExecInfo,
        file::{File, FileDescription},
    },
};
use alloc::{
    boxed::Box,
    collections::btree_map::BTreeMap,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{
    mem,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    time::Duration,
};

#[derive(Debug)]
pub enum State {
    Running,
    Stopped(Signal),
    Exited(u8),
    Signaled(Signal),
}

pub struct Process {
    /// The unique identifier of this process.
    id: uapi::pid_t,
    /// The display name of this process.
    name: SpinMutex<String>,
    /// The parent of this process, or [`None`], if this is the init process.
    parent: SpinMutex<Option<Weak<Process>>>,
    /// A list of [`Task`]s associated with this process.
    pub threads: SpinMutex<Vec<Arc<Task>>>,
    /// The address space for this process.
    pub address_space: SpinMutex<Arc<Mutex<AddressSpace>>>,
    /// The root directory for this process.
    pub root_dir: SpinMutex<PathNode>,
    /// Current working directory.
    pub working_dir: SpinMutex<PathNode>,
    /// The status of this process.
    pub status: SpinMutex<State>,
    /// Child processes owned by this process.
    pub children: SpinMutex<Vec<Arc<Process>>>,
    /// The user identity of this process.
    pub identity: SpinMutex<Identity>,
    /// A table of open file descriptors.
    pub open_files: SpinMutex<FdTable>,
    /// Per-process signal action table.
    pub signal_actions: SpinMutex<SignalState>,
    /// Process group ID.
    pub pgrp: SpinMutex<uapi::pid_t>,
    /// Session ID.
    pub session: SpinMutex<uapi::pid_t>,
    /// Controlling terminal, if any.
    pub controlling_tty: SpinMutex<Option<Arc<Tty>>>,
    /// Process-wide real-time interval timer.
    pub real_timer: SpinMutex<IntervalTimerState>,
    /// Event that is signalled when a child process changes state
    /// (exited, signaled, stopped, or continued).
    pub child_event: Event,
    /// Event used by stopped threads to wait for SIGCONT.
    pub cont_event: Event,
    /// Event signalled whenever a signal is queued to a thread of this process.
    /// Used by signalfd readers to wake up when new signals arrive.
    pub signalfd_event: Event,
    /// Latched when this process transitions into Stopped; cleared when a
    /// waiter observes it via WUNTRACED.
    pub stop_unwaited: AtomicBool,
    /// Latched when a stopped process is continued; cleared when a waiter
    /// observes it via WCONTINUED.
    pub continue_unwaited: AtomicBool,
    /// File mode creation mask.
    pub umask: AtomicU32,
}

impl Process {
    /// Returns the unique identifier of this process.
    #[inline]
    pub const fn get_pid(&self) -> uapi::pid_t {
        self.id
    }

    pub fn get_name(&self) -> String {
        self.name.lock().clone()
    }

    pub fn set_name(&self, name: String) {
        *self.name.lock() = name;
    }

    pub fn get_real_timer(&self, now: Duration) -> uapi::time::itimerval {
        self.real_timer.lock().snapshot(now)
    }

    pub fn set_real_timer(
        &self,
        now: Duration,
        value: uapi::time::itimerval,
    ) -> EResult<uapi::time::itimerval> {
        self.real_timer.lock().replace(now, value)
    }

    /// Gets the parent process of this process.
    /// Returns [`None`], if it is the init process.
    pub fn get_parent(&self) -> Option<Arc<Self>> {
        self.parent.lock().as_ref().and_then(Weak::upgrade)
    }

    pub fn new(name: String, parent: Option<Arc<Self>>) -> EResult<Self> {
        Self::new_with_space(name, parent, AddressSpace::new())
    }

    fn replace_address_space(
        &self,
        address_space: Arc<Mutex<AddressSpace>>,
    ) -> Arc<Mutex<AddressSpace>> {
        mem::replace(&mut *self.address_space.lock(), address_space)
    }

    pub fn fork(self: Arc<Self>, context: &Context) -> EResult<(Arc<Self>, Arc<Task>)> {
        let forked_space = {
            let parent_space = self.address_space.lock().clone();
            let (forked, shoot) = {
                let parent_guard = parent_space.lock();
                let (forked, range) = parent_guard.fork()?;
                let table = parent_guard.table.clone();
                (forked, range.map(|(addr, len)| (table, addr, len)))
            };
            // Flush the parent's newly copy-on-write pages with the address-space
            // lock released (the shootdown may block waiting for remote CPUs).
            if let Some((table, addr, len)) = shoot {
                crate::memory::virt::shootdown::submit_shootdown(&table, addr.value(), len);
            }
            Arc::new(Mutex::new(forked))
        };
        let forked = Arc::new(Self {
            id: PID_COUNTER.fetch_add(1, Ordering::Acquire) as _,
            name: SpinMutex::new(self.name.lock().clone()),
            parent: SpinMutex::new(Some(Arc::downgrade(&self))),
            threads: SpinMutex::new(Vec::new()),
            address_space: SpinMutex::new(forked_space),
            root_dir: SpinMutex::new(self.root_dir.lock().clone()),
            working_dir: SpinMutex::new(self.working_dir.lock().clone()),
            status: SpinMutex::new(State::Running),
            children: SpinMutex::new(Vec::new()),
            identity: SpinMutex::new(self.identity.lock().clone()),
            open_files: SpinMutex::new(self.open_files.lock().clone()),
            signal_actions: SpinMutex::new(self.signal_actions.lock().clone()),
            pgrp: SpinMutex::new(*self.pgrp.lock()),
            session: SpinMutex::new(*self.session.lock()),
            controlling_tty: SpinMutex::new(self.controlling_tty.lock().clone()),
            real_timer: SpinMutex::new(*self.real_timer.lock()),
            child_event: Event::new(),
            cont_event: Event::new(),
            signalfd_event: Event::new(),
            stop_unwaited: AtomicBool::new(false),
            continue_unwaited: AtomicBool::new(false),
            umask: AtomicU32::new(self.umask.load(Ordering::Relaxed)),
        });

        // Create a heap allocated context that we can pass to the entry point.
        let mut forked_ctx = Box::new(*context);
        forked_ctx.set_return(0, 0); // User mode returns 0 for forked processes.
        let raw_ctx = Box::into_raw(forked_ctx);

        // Create the main thread.
        let forked_thread = Arc::new(Task::new(to_user_context, raw_ctx as _, 0, &forked, true)?);
        forked_thread.signal.lock().mask = Scheduler::get_current().signal.lock().mask;
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

        let id = PID_COUNTER.fetch_add(1, Ordering::Relaxed) as _;
        // For the very first processes (kernel, init) pgrp/session default to their own PID.
        let pgrp = if pgrp == 0 { id } else { pgrp };
        let session = if session == 0 { id } else { session };

        Ok(Self {
            id,
            name: SpinMutex::new(name),
            parent: SpinMutex::new(parent.map(|x| Arc::downgrade(&x))),
            threads: SpinMutex::new(Vec::new()),
            address_space: SpinMutex::new(Arc::new(Mutex::new(space))),
            status: SpinMutex::new(State::Running),
            children: SpinMutex::new(Vec::new()),
            root_dir: SpinMutex::new(root),
            working_dir: SpinMutex::new(cwd),
            identity: SpinMutex::new(identity),
            open_files: SpinMutex::new(FdTable::new()),
            signal_actions: SpinMutex::new(SignalState::new()),
            pgrp: SpinMutex::new(pgrp),
            session: SpinMutex::new(session),
            controlling_tty: SpinMutex::new(ctty),
            real_timer: SpinMutex::new(IntervalTimerState::default()),
            child_event: Event::new(),
            cont_event: Event::new(),
            signalfd_event: Event::new(),
            stop_unwaited: AtomicBool::new(false),
            continue_unwaited: AtomicBool::new(false),
            umask: AtomicU32::new(0o022),
        })
    }

    /// Returns the kernel process.
    pub fn get_kernel() -> &'static Arc<Self> {
        KERNEL_PROCESS.get()
    }

    /// Replaces a process with a new executable image, given some arguments and an environment.
    /// The given file must be opened with ReadOnly and Executable.
    /// Any existing threads of this process are destroyed upon a successful execve.
    /// If this is the current process, a successful execve will never return.
    pub fn fexecve(
        self: Arc<Self>,
        file: Arc<File>,
        exec_path: Vec<u8>,
        argv: Vec<Vec<u8>>,
        envp: Vec<Vec<u8>>,
    ) -> EResult<()> {
        let current = Scheduler::get_current();
        let is_current_process = Arc::ptr_eq(&current.get_process(), &self);
        let old_threads;

        {
            let mut info = ExecInfo {
                executable: file.clone(),
                interpreter: None,
                space: AddressSpace::new(),
                exec_path,
                argv,
                envp,
            };

            let format = vfs::exec::identify(&file).ok_or(Errno::ENOEXEC)?;
            let mut init_task = format.load(&self, &mut info)?;
            let new_address_space = Arc::new(Mutex::new(info.space));
            init_task.address_space = new_address_space.clone();
            let init = Arc::try_new(init_task)?;

            // Adopt the new executable's basename as the name.
            if let Some(basename) = info
                .exec_path
                .rsplit(|&b| b == b'/')
                .next()
                .filter(|s| !s.is_empty())
            {
                self.set_name(String::from_utf8_lossy(basename).into_owned());
            }

            let mut threads = self.threads.lock();
            old_threads = mem::take(&mut *threads);
            for thread in &old_threads {
                if is_current_process && Arc::ptr_eq(thread, &current) {
                    continue;
                }
                *thread.state.lock() = task::State::Dead;
            }
            threads.push(init.clone());

            let old_space = self.replace_address_space(new_address_space.clone());
            let new_table = new_address_space.lock().table.clone();

            let closed = self.open_files.lock().close_exec();
            drop(closed);
            self.signal_actions.lock().reset_on_exec();

            if is_current_process {
                unsafe { new_table.set_active() };
            }
            drop(old_space);
            CpuData::get().scheduler.add_task(init);
        }

        drop(file);
        drop(self);

        if is_current_process {
            drop(old_threads);
            drop(current);

            // execve never returns on success when replacing the current process.
            Scheduler::kill_current();
        }

        Ok(())
    }

    /// Exits the current process.
    pub fn exit(new_state: State) -> ! {
        let task = Scheduler::get_current();
        let proc = task.get_process();
        let pid = proc.get_pid();

        if pid <= 1 {
            panic!("Attempted to kill init with process state {:?}", new_state);
        }

        PROCESS_TABLE.lock().remove(&proc.get_pid());
        proc.real_timer.lock().disarm();

        let old_space = proc.replace_address_space(Arc::new(Mutex::new(AddressSpace::new_kernel(
            KERNEL_PAGE_TABLE.get().clone(),
        ))));

        {
            let mut open_files = proc.open_files.lock();
            let mut threads = proc.threads.lock();
            let mut status = proc.status.lock();

            // Kill all other threads. Keep the current task runnable until the parent has been notified below.
            for thread in threads.iter() {
                if !Arc::ptr_eq(thread, &task) {
                    *thread.state.lock() = task::State::Dead;
                }
            }
            threads.clear();

            // Close all files.
            let closed_files = open_files.close_all();

            *status = new_state;

            drop(status);
            drop(threads);
            drop(open_files);
            drop(closed_files);
        }

        drop(old_space);

        // Reparent orphaned children to the init process.
        {
            let mut our_children = proc.children.lock();
            if !our_children.is_empty() {
                let init = INIT.get();
                let mut init_children = init.children.lock();
                for child in our_children.drain(..) {
                    *child.parent.lock() = Some(Arc::downgrade(init));
                    init_children.push(child);
                }
                // Wake init in case any of these children are already exited.
                init.child_event.wake_all();
            }
        }

        let (cld_code, cld_status) = match *proc.status.lock() {
            State::Exited(code) => (uapi::signal::CLD_EXITED as i32, code as i32),
            State::Signaled(sig) if sig.default_action() == signal::DefaultAction::CoreDump => {
                (uapi::signal::CLD_DUMPED as i32, sig as i32)
            }
            State::Signaled(sig) => (uapi::signal::CLD_KILLED as i32, sig as i32),
            _ => (uapi::signal::CLD_EXITED as i32, 0),
        };
        signal::notify_parent_of_child_state_change(&proc, cld_code, cld_status);

        drop(proc);
        drop(task);

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

    /// Updates the `close_on_exec` flag on the file descriptor in place.
    pub fn set_close_on_exec(&mut self, fd: i32, value: bool) -> Option<()> {
        if fd.is_negative() {
            return None;
        }
        let desc = self.inner.get(&fd)?;
        desc.close_on_exec.store(value, Ordering::Release);
        Some(())
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

    pub fn close(&mut self, fd: i32) -> Option<FileDescription> {
        self.inner.remove(&fd)
    }

    pub fn close_all(&mut self) -> Vec<FileDescription> {
        core::mem::take(&mut self.inner).into_values().collect()
    }

    /// Closes all files with the [`OpenFlags::CloseOnExec`] flag.
    pub fn close_exec(&mut self) -> Vec<FileDescription> {
        let fds = self
            .inner
            .iter()
            .filter(|(_, desc)| desc.close_on_exec.load(Ordering::Acquire))
            .map(|(fd, _)| *fd)
            .collect::<Vec<_>>();

        fds.into_iter()
            .filter_map(|fd| self.inner.remove(&fd))
            .collect()
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

    /// Supplementary group IDs.
    pub groups: Vec<uapi::gid_t>,
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
            groups: Vec::new(),
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
pub static PROCESS_TABLE: SpinMutex<BTreeMap<uapi::pid_t, Weak<Process>>> =
    SpinMutex::new(BTreeMap::new());

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
                AddressSpace::new_kernel(super::memory::virt::KERNEL_PAGE_TABLE.get().clone()),
            )
            .expect("Unable to create the main kernel process"),
        ))
    };

    let kproc = KERNEL_PROCESS.get();
    PROCESS_TABLE
        .lock()
        .insert(kproc.get_pid(), Arc::downgrade(&kproc));
}
