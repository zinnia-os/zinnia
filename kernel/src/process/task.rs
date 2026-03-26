use super::Process;
use crate::{
    arch::{self},
    memory::{UserAccessRegion, virt::KERNEL_STACK_SIZE},
    posix::errno::EResult,
    util::mutex::spin::SpinMutex,
};
use alloc::sync::{Arc, Weak};
use core::{
    alloc::Layout,
    panic,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TaskState {
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
#[derive(Debug)]
pub struct Task {
    /// The unique identifier of this task.
    id: Tid,
    /// The process which this task belongs to.
    process: Weak<Process>,
    /// If this task is a user task. `false` forbids this task to ever enter user mode.
    is_user: bool,
    /// The current state of the thread.
    pub state: SpinMutex<TaskState>,
    /// The saved context of a task while it is not running.
    pub task_context: SpinMutex<arch::sched::TaskContext>,
    /// The kernel stack for this task.
    pub kernel_stack: AtomicUsize,
    /// The user stack for this task.
    pub user_stack: AtomicUsize,
    /// The amount of time that this task can live on.
    pub ticks: usize,
    /// A value between -20 and 19, where -20 is the highest priority and 0 is a neutral priority.
    pub priority: i8,
    /// Used to handle [`UserPtr`] page faults.
    pub uar: AtomicPtr<UserAccessRegion>,
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
        let kernel_stack =
            AtomicUsize::new(unsafe { alloc::alloc::alloc_zeroed(STACK_LAYOUT) as usize });

        let result = Self {
            id: TASK_ID_COUNTER.fetch_add(1, Ordering::Acquire),
            is_user,
            process: Arc::downgrade(parent),
            uar: AtomicPtr::new(null_mut()),
            state: SpinMutex::new(TaskState::Ready),
            task_context: SpinMutex::new(arch::sched::TaskContext::default()),
            kernel_stack,
            user_stack: AtomicUsize::new(0),
            ticks: 0,
            priority: 0,
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
        if let Some(x) = self.process.upgrade() {
            x
        } else {
            todo!()
        }
    }
}

/// Global counter to provide new task IDs.
static TASK_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);
