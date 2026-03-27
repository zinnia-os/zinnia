pub mod signal;
pub mod task;

use crate::{
    arch::sched::Context,
    memory::{VirtAddr, virt::AddressSpace},
    percpu::CpuData,
    posix::errno::{EResult, Errno},
    process::task::Task,
    sched::Scheduler,
    uapi,
    util::{mutex::spin::SpinMutex, once::Once},
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
    /// A pointer to the next free memory region.
    pub mmap_head: SpinMutex<VirtAddr>,
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
            mmap_head: SpinMutex::new(*self.mmap_head.lock()),
        });

        // Create a heap allocated context that we can pass to the entry point.
        let mut forked_ctx = Box::new(*context);
        forked_ctx.set_return(0, 0); // User mode returns 0 for forked processes.
        let raw_ctx = Box::into_raw(forked_ctx);

        // Create the main thread.
        let forked_thread = Arc::new(Task::new(to_user_context, raw_ctx as _, 0, &forked, true)?);
        forked.threads.lock().push(forked_thread.clone());
        self.children.lock().push(forked.clone());

        Ok((forked, forked_thread))
    }

    fn new_with_space(
        name: String,
        parent: Option<Arc<Self>>,
        space: AddressSpace,
    ) -> EResult<Self> {
        let (root, cwd, identity) = match &parent {
            Some(x) => (
                x.root_dir.lock().clone(),
                x.working_dir.lock().clone(),
                x.identity.lock().clone(),
            ),
            None => (vfs::get_root(), vfs::get_root(), Identity::default()),
        };

        // Save the child in the parent process.
        if let Some(x) = &parent {
            x.children.lock().push(x.clone())
        }

        Ok(Self {
            id: PID_COUNTER.fetch_add(1, Ordering::Relaxed),
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
            // TODO: This address should be determined from the highest loaded segment.
            mmap_head: SpinMutex::new(VirtAddr::new(0x1_0000_0000)),
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
        }

        CpuData::get().scheduler.add_task(init);

        // execve never returns on success.
        Scheduler::kill_current();
    }

    pub fn exit(self: Arc<Self>, code: u8) {
        {
            let mut open_files = self.open_files.lock();
            let mut threads = self.threads.lock();
            let mut status = self.status.lock();

            // Kill all threads.
            for thread in threads.iter() {
                *thread.state.lock() = task::TaskState::Dead;
            }
            threads.clear();

            // Close all files.
            open_files.close_all();

            *status = ProcessState::Exited(code);
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
            groups: vec![],
        };
        &KERNEL_IDENTITY
    }
}

static PID_COUNTER: AtomicUsize = AtomicUsize::new(0);
static KERNEL_PROCESS: Once<Arc<Process>> = Once::new();

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
}
