use crate::{
    arch::{self},
    memory::{
        UserAccessRegion,
        virt::{AddressSpace, KERNEL_STACK_SIZE},
    },
    posix::errno::EResult,
    process::Process,
    process::signal::ThreadSignalState,
    util::mutex::spin::SpinMutex,
};
use alloc::{string::String, sync::Arc};
use core::{
    alloc::Layout,
    fmt::Debug,
    panic,
    ptr::null_mut,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering},
};

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
    pub address_space: Arc<SpinMutex<AddressSpace>>,
    /// The current state of the thread.
    pub state: SpinMutex<State>,
    /// Whether this task is currently present in some scheduler run queue.
    /// Used by the scheduler to deduplicate enqueues from concurrent wakeups.
    pub queued: AtomicBool,
    /// Set by [`Scheduler::add_task`] when a wakeup arrives while this task is still `Running`.
    pub wake_pending: AtomicBool,
    /// The saved context of a task while it is not running.
    pub task_context: SpinMutex<arch::sched::TaskContext>,
    /// Allocation base of the kernel stack for this task.
    pub kernel_stack: AtomicUsize,
    /// Stack pointer used when entering the kernel from userspace.
    pub kernel_entry_stack: AtomicUsize,
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
    /// Run-queue bucket this task was most recently placed in.
    pub queued_bucket: AtomicUsize,
    /// Whether this task may be moved between CPUs by balancing or stealing.
    pub migration_enabled: AtomicBool,
    /// Whether this task is counted in its assigned CPU's runnable load.
    pub load_counted: AtomicBool,
}

const STACK_LAYOUT: Layout = match Layout::from_size_align(KERNEL_STACK_SIZE, 0x1000) {
    Ok(x) => x,
    Err(_) => panic!("Layout error"),
};

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
        address_space: Arc<SpinMutex<AddressSpace>>,
        is_user: bool,
    ) -> EResult<Self> {
        let kernel_stack_base = unsafe { alloc::alloc::alloc_zeroed(STACK_LAYOUT) as usize };
        let kernel_stack = AtomicUsize::new(kernel_stack_base);

        let result = Self {
            id: TASK_ID_COUNTER.fetch_add(1, Ordering::Acquire),
            is_user,
            process: parent.clone(),
            address_space,
            uar: AtomicPtr::new(null_mut()),
            signal: SpinMutex::new(ThreadSignalState::default()),
            state: SpinMutex::new(State::Ready),
            queued: AtomicBool::new(false),
            wake_pending: AtomicBool::new(false),
            task_context: SpinMutex::new(arch::sched::TaskContext::default()),
            kernel_stack,
            kernel_entry_stack: AtomicUsize::new(kernel_stack_base + KERNEL_STACK_SIZE),
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
            queued_bucket: AtomicUsize::new(usize::MAX),
            migration_enabled: AtomicBool::new(true),
            load_counted: AtomicBool::new(false),
        };

        {
            let mut task_context = result.task_context.lock();
            arch::sched::init_task(
                &mut task_context,
                entry,
                arg1,
                arg2,
                result.kernel_stack.load(Ordering::Relaxed).into(),
                is_user,
            )?;
        }

        return Ok(result);
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
}

impl Drop for Task {
    fn drop(&mut self) {
        let stack = self.kernel_stack.load(Ordering::Relaxed);
        if stack != 0 {
            unsafe { alloc::alloc::dealloc(stack as *mut u8, STACK_LAYOUT) };
        }
    }
}

/// Global counter to provide new task IDs.
static TASK_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);
