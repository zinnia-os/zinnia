use crate::{
    arch::{self},
    irq::lock::{IrqGuard, IrqLock},
    percpu::{CPU_DATA, CpuData},
    process::{
        Process,
        task::{Task, TaskState},
    },
    util::mutex::spin::SpinMutex,
};
use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use core::{
    mem,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

/// An instance of a scheduler. Each CPU has one instance running to coordinate task management.
#[derive(Debug)]
pub struct Scheduler {
    /// The currently running task on this scheduler instance. Use [`Self::get_current`] instead.
    pub(crate) current: AtomicPtr<Task>,
    pub(crate) idle_task: AtomicPtr<Task>,
    pub(crate) preempt_level: usize,
    run_queue: SpinMutex<VecDeque<Arc<Task>>>,
}

impl Scheduler {
    pub(crate) const fn new() -> Self {
        return Self {
            current: AtomicPtr::new(null_mut()),
            idle_task: AtomicPtr::new(null_mut()),
            preempt_level: 0,
            run_queue: SpinMutex::new(VecDeque::new()),
        };
    }

    /// Adds a task to a run queue.
    pub fn add_task(&self, task: Arc<Task>) {
        self.run_queue.lock().push_back(task);
    }

    fn choose_target_cpu() -> &'static CpuData {
        let current_cpu = CpuData::get();
        let mut min_load = usize::MAX;
        let mut least_loaded_cpu = current_cpu;

        for cpu_data in CpuData::iter() {
            if cpu_data.id != current_cpu.id && !cpu_data.online.load(Ordering::Acquire) {
                continue;
            }

            let load = cpu_data.scheduler.run_queue.lock().len();
            if load < min_load {
                min_load = load;
                least_loaded_cpu = cpu_data;
            }
        }

        least_loaded_cpu
    }

    /// Adds a task to the run queue of the CPU with the lowest load.
    /// This is used for new process creation to balance load across CPUs.
    pub fn add_task_to_best_cpu(task: Arc<Task>) {
        let least_loaded_cpu = Self::choose_target_cpu();
        least_loaded_cpu.scheduler.add_task(task);
    }

    pub fn wake_task(task: Arc<Task>) {
        let current_cpu = CpuData::get();
        let target_cpu = Self::choose_target_cpu();
        target_cpu.scheduler.add_task(task);

        if target_cpu.id != current_cpu.id {
            unsafe { arch::sched::remote_reschedule(target_cpu.id as u32) };
        }
    }

    /// Returns the task currently running on this CPU.
    pub fn get_current() -> Arc<Task> {
        let ptr = CPU_DATA.get().scheduler.current.load(Ordering::Acquire);
        debug_assert!(!ptr.is_null());

        // If we don't do this, then the Arc's refcount won't get incremented.
        let task = unsafe { Arc::from_raw(ptr) };
        let result = task.clone();
        mem::forget(task);
        result
    }

    fn next(&self) -> Option<Arc<Task>> {
        let mut queue = self.run_queue.lock();
        while let Some(x) = &queue.pop_front() {
            let inner = x.state.lock();
            if *inner == TaskState::Ready {
                return Some(x.clone());
            }
        }
        None
    }

    /// Puts the current task back to the run queue and reschedules.
    pub fn reschedule(&self) {
        let lock = IrqLock::lock();
        let from = self.current.load(Ordering::Acquire);

        if from != self.idle_task.load(Ordering::Acquire) {
            self.add_task(unsafe {
                let task = Arc::from_raw(from);
                let result = task.clone();
                mem::forget(task);
                result
            });
        }

        self.do_reschedule(lock);
    }

    /// Reschedules without adding the current task back to the run queue.
    pub fn do_yield(&self) {
        let lock = IrqLock::lock();
        self.do_reschedule(lock);
    }

    /// Runs the scheduler.
    fn do_reschedule(&self, irq_guard: IrqGuard) {
        let from = self.current.load(Ordering::Acquire);
        let to = self
            .next()
            .map(|task| Arc::into_raw(task) as *mut _)
            .unwrap_or(self.idle_task.load(Ordering::Acquire));

        if from == to {
            return;
        }

        self.current.store(to, Ordering::Relaxed);

        unsafe {
            let to_proc = (*to).get_process();

            // If we are switching between address spaces, we need to update the page table.
            // TODO: This is very ugly.
            (*to_proc.address_space.raw_inner()).table.set_active();

            let cpu = CPU_DATA.get();

            {
                // Save the current kernel and user stack pointers to the old task.
                (*from)
                    .kernel_stack
                    .store(cpu.kernel_stack.load(Ordering::Acquire), Ordering::Release);
                (*from)
                    .user_stack
                    .store(cpu.user_stack.load(Ordering::Acquire), Ordering::Release);

                // Get the kernel and user stack pointers from the new task and write them to the per-CPU data.
                cpu.kernel_stack.store(
                    (*to).kernel_stack.load(Ordering::Acquire),
                    Ordering::Release,
                );
                cpu.user_stack
                    .store((*to).user_stack.load(Ordering::Acquire), Ordering::Release);
            }

            arch::sched::switch(from, to, irq_guard);
        }
    }

    /// Kills the currently running task.
    pub fn kill_current() -> ! {
        let task = Scheduler::get_current();
        *task.state.lock() = TaskState::Dead;
        CPU_DATA.get().scheduler.do_yield();
        unreachable!("The scheduler did not kill this task");
    }

    fn set_task(&self, task: Arc<Task>) {
        let new_ptr = Arc::into_raw(task);
        let old_ptr = self.current.swap(new_ptr as *mut _, Ordering::AcqRel);
        if !old_ptr.is_null() {
            _ = unsafe { Arc::from_raw(old_ptr) }; // Arc is dropped here.
        }
    }
}

/// Generic task entry point. This is to be called by an implementing [`crate::arch::sched::init_task`].
pub extern "C" fn task_entry(entry: extern "C" fn(usize, usize), arg1: usize, arg2: usize) -> ! {
    (entry)(arg1, arg2);

    // The task function is over, kill the task.
    Scheduler::kill_current();
}

/// Function used for waiting.
pub extern "C" fn idle_fn(_: usize, _: usize) {
    loop {
        crate::arch::irq::wait_for_irq();
    }
}

pub extern "C" fn dummy_fn(_: usize, _: usize) {
    unreachable!("Tried to actually run a dummy task");
}

#[initgraph::task(
    name = "generic.scheduler",
    depends = [crate::memory::MEMORY_STAGE, super::process::PROCESS_STAGE],
)]
pub fn SCHEDULER_STAGE() {
    // Set up scheduler.
    let bsp = &CpuData::get().scheduler;
    let idle_task = Arc::new(Task::new(idle_fn, 0, 0, Process::get_kernel(), false).unwrap());

    // Create a new idle task.
    bsp.idle_task
        .store(Arc::into_raw(idle_task) as *mut _, Ordering::Release);

    // Create a dummy task to drop right after the first reschedule.
    let dummy = Arc::new(Task::new(dummy_fn, 0, 0, Process::get_kernel(), false).unwrap());

    // Add the main function as the first task.
    let initial_task =
        Arc::new(Task::new(crate::main, 0, 0, Process::get_kernel(), false).unwrap());
    bsp.add_task(initial_task);
    bsp.set_task(dummy);
}
