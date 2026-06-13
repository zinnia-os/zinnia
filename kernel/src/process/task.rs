use crate::{
    arch::{self},
    memory::{UserAccessRegion, stack::KernelStack, virt::AddressSpace},
    percpu::CpuData,
    posix::errno::EResult,
    process::{Process, signal::ThreadSignalState},
    sched::Scheduler,
    util::mutex::{Mutex, spin::SpinMutex},
};
use alloc::{
    boxed::Box,
    string::String,
    sync::{Arc, Weak},
    task::Wake,
};
use core::{
    cell::SyncUnsafeCell,
    fmt::Debug,
    ptr::null_mut,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering},
    task::Waker,
};
use intrusive_collections::LinkedListAtomicLink;

const UNBLOCKED_BIT: usize = 1;
const NEXT_BLOCK_TOKEN: usize = 1 << 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockToken(usize);

impl BlockToken {
    pub const fn value(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Currently being executed.
    Running,
    /// Ready to run.
    Ready,
    /// Waiting for a timer or another signal.
    Waiting,
    /// Task is being killed.
    Dying,
    /// Task is killed and waiting for cleanup.
    Dead,
}

pub type Tid = usize;

/// Represents the atomic scheduling structure.
pub struct Task {
    /// The unique identifier of this task.
    id: Tid,
    /// The process which this task belongs to.
    process: Arc<Process>,
    /// If this task is a user task. `false` forbids this task to ever enter user mode.
    is_user: bool,
    /// The address space this task executes in.
    pub address_space: Arc<Mutex<AddressSpace>>,
    /// The current state of the thread.
    pub state: SpinMutex<State>,
    /// Whether this task is currently present in some scheduler run queue.
    /// Used by the scheduler to deduplicate enqueues from concurrent wakeups.
    pub queued: AtomicBool,
    /// Set by [`Scheduler::add_task`] when a wakeup arrives while this task is still `Running`.
    pub wake_pending: AtomicBool,
    /// True while this task's saved context is owned by a CPU.
    pub on_cpu: AtomicBool,
    /// Token used to pair async wakers with the block cycle that created them.
    block_token: AtomicUsize,
    /// Saved arch context. Touched only by the CPU running this task
    /// (and by `init_context` before the task is published).
    pub executor: SyncUnsafeCell<arch::sched::Executor>,
    /// Allocation base of the kernel stack for this task.
    pub kernel_stack: KernelStack,
    /// The user stack for this task.
    pub user_stack: AtomicUsize,
    /// The amount of time that this task can live on.
    pub ticks: usize,
    /// A value between -20 and 19, where -20 is the highest priority and 0 is a neutral priority.
    pub priority: i8,
    /// Used to handle [`UserPtr`] page faults.
    pub uar: AtomicPtr<UserAccessRegion>,
    /// Per-thread signal state (pending signals and signal mask).
    pub signal: SpinMutex<ThreadSignalState>,
    /// The display name of this thread.
    pub name: SpinMutex<String>,
    /// CPU id this task last ran on.
    pub last_cpu: AtomicU32,
    /// CPU id this task is currently assigned to, or [`u32::MAX`] if none.
    pub sched_cpu: AtomicU32,
    /// Last scheduler tick at which this task was selected to run.
    pub last_run_tick: AtomicUsize,
    /// Scheduler tick at which this task most recently went to sleep.
    pub sleep_started_tick: AtomicUsize,
    /// Runtime accumulated for scoring.
    pub sched_runtime: AtomicUsize,
    /// Sleep time accumulated for scoring.
    pub sched_sleeptime: AtomicUsize,
    /// Ticks consumed in the current time slice.
    pub sched_slice: AtomicUsize,
    /// Current dynamic scheduler priority.
    pub dynamic_priority: AtomicUsize,
    /// Whether this task may be moved between CPUs by balancing or stealing.
    pub migration_enabled: AtomicBool,
    /// Whether this task is permanently bound to its owning CPU and must never
    /// be migrated or stolen.
    pub bound: AtomicBool,
    /// Whether this task is counted in its assigned CPU's runnable load.
    pub load_counted: AtomicBool,
    /// Intrusive link into a CPU run queue.
    pub run_link: LinkedListAtomicLink,
    /// Intrusive link into a CPU reap queue.
    pub reap_link: LinkedListAtomicLink,
}

impl Task {
    /// Creates a new task.
    pub fn new(
        entry: extern "C" fn(usize, usize),
        arg1: usize,
        arg2: usize,
        parent: &Arc<Process>,
        is_user: bool,
    ) -> EResult<Self> {
        Self::new_in_address_space(
            entry,
            arg1,
            arg2,
            parent,
            parent.address_space.lock().clone(),
            is_user,
        )
    }

    /// Creates a new task in a specific address space.
    pub fn new_in_address_space(
        entry: extern "C" fn(usize, usize),
        arg1: usize,
        arg2: usize,
        parent: &Arc<Process>,
        address_space: Arc<Mutex<AddressSpace>>,
        is_user: bool,
    ) -> EResult<Self> {
        let result = Self::new_uninitialized(parent, address_space, is_user)?;
        result.init_context(entry, arg1, arg2, is_user)?;

        return Ok(result);
    }

    /// Creates a new kernel task that runs a Rust closure.
    pub fn run<F>(f: F) -> EResult<Arc<Self>>
    where
        F: FnOnce(Arc<Self>) + Send + 'static,
    {
        extern "C" fn task_entry<F: FnOnce(Arc<Task>) + Send + 'static>(f: usize, task: usize) {
            let task = unsafe { Weak::from_raw(task as *const Task).upgrade().unwrap() };
            let f = unsafe { Box::from_raw(f as *mut F) };

            f(task);
        }

        let proc = Process::get_kernel();
        let result = Arc::try_new(Self::new_uninitialized(
            &proc,
            proc.address_space.lock().clone(),
            false,
        )?)?;
        let f = Box::into_raw(Box::try_new(f)?);
        let task = Weak::into_raw(Arc::downgrade(&result));

        if let Err(error) = result.init_context(task_entry::<F>, f as usize, task as usize, false) {
            unsafe {
                drop(Box::from_raw(f));
                drop(Weak::from_raw(task));
            }
            return Err(error);
        }

        Scheduler::add_task_to_best_cpu(result.clone());
        Ok(result)
    }

    pub fn run_async<F>(f: F) -> EResult<Arc<Self>>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self::run(move |_| CpuData::get().scheduler.block_on(f))
    }

    fn new_uninitialized(
        parent: &Arc<Process>,
        address_space: Arc<Mutex<AddressSpace>>,
        is_user: bool,
    ) -> EResult<Self> {
        Ok(Self {
            id: TASK_ID_COUNTER.fetch_add(1, Ordering::Acquire),
            is_user,
            process: parent.clone(),
            address_space,
            uar: AtomicPtr::new(null_mut()),
            signal: SpinMutex::new(ThreadSignalState::default()),
            state: SpinMutex::new(State::Ready),
            queued: AtomicBool::new(false),
            wake_pending: AtomicBool::new(false),
            on_cpu: AtomicBool::new(false),
            block_token: AtomicUsize::new(0),
            executor: SyncUnsafeCell::new(arch::sched::Executor::default()),
            kernel_stack: KernelStack::new()?,
            user_stack: AtomicUsize::new(0),
            ticks: 0,
            priority: 0,
            name: SpinMutex::new(String::new()),
            last_cpu: AtomicU32::new(u32::MAX),
            sched_cpu: AtomicU32::new(u32::MAX),
            last_run_tick: AtomicUsize::new(0),
            sleep_started_tick: AtomicUsize::new(0),
            sched_runtime: AtomicUsize::new(0),
            sched_sleeptime: AtomicUsize::new(0),
            sched_slice: AtomicUsize::new(0),
            dynamic_priority: AtomicUsize::new(usize::MAX),
            migration_enabled: AtomicBool::new(true),
            bound: AtomicBool::new(false),
            load_counted: AtomicBool::new(false),
            run_link: LinkedListAtomicLink::new(),
            reap_link: LinkedListAtomicLink::new(),
        })
    }

    fn init_context(
        &self,
        entry: extern "C" fn(usize, usize),
        arg1: usize,
        arg2: usize,
        is_user: bool,
    ) -> EResult<()> {
        // SAFETY: caller has exclusive access; task is not yet published.
        let executor = unsafe { &mut *self.executor.get() };
        arch::sched::init_task(executor, entry, arg1, arg2, &self.kernel_stack, is_user)
    }

    /// Returns true if this is a user task.
    #[inline]
    pub const fn is_user(&self) -> bool {
        self.is_user
    }

    /// Returns the ID of this task.
    #[inline]
    pub const fn get_id(&self) -> Tid {
        self.id
    }

    /// Returns the process which this task belongs to.
    #[inline]
    pub fn get_process(&self) -> Arc<Process> {
        self.process.clone()
    }

    /// Returns true if this task has any pending signals that are not blocked.
    pub fn has_pending_signals(&self) -> bool {
        let state = self.signal.lock();
        !(state.pending & !state.mask).is_empty()
    }

    pub(crate) fn next_block_token(&self) -> BlockToken {
        let mut result = 0;
        self.block_token
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                result = (old & !UNBLOCKED_BIT).wrapping_add(NEXT_BLOCK_TOKEN);
                Some(result)
            })
            .unwrap();
        BlockToken(result)
    }

    pub(crate) fn unblock(self: &Arc<Self>, token: BlockToken) {
        if self
            .block_token
            .compare_exchange(
                token.value(),
                token.value() | UNBLOCKED_BIT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            Scheduler::wake_task(self.clone());
        }
    }

    pub(crate) fn waker(self: &Arc<Self>, token: BlockToken) -> Waker {
        Waker::from(Arc::new(TaskWaker {
            task: self.clone(),
            token,
        }))
    }
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        Scheduler::wake_task(self);
    }
}

struct TaskWaker {
    task: Arc<Task>,
    token: BlockToken,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.task.unblock(self.token);
    }
}

/// Global counter to provide new task IDs.
static TASK_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);
