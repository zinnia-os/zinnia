use crate::{
    arch::{self},
    irq::lock::{IrqGuard, IrqLock},
    percpu::{CPU_DATA, CpuData},
    process::{
        Process,
        task::{State, Task},
    },
    util::mutex::spin::SpinMutex,
};
use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use core::{
    mem,
    ptr::{addr_eq, null_mut},
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering},
};

const NO_CPU: u32 = u32::MAX;
const NO_BUCKET: usize = usize::MAX;
const BASE_SLICE_TICKS: usize = 10;
const MIN_SLICE_TICKS: usize = 1;
const AFFINITY_TICKS: usize = 4;
const STEAL_THRESHOLD: usize = 2;
const SLEEP_RUN_MAX: usize = 512;

const INTERACT_MAX: usize = 100;
const INTERACT_THRESH: usize = 30;
const INTERACTIVE_BUCKETS: usize = INTERACT_THRESH;
const TIMESHARE_BUCKETS: usize = 32;

const BUCKET_KERNEL: usize = 0;
const BUCKET_INTERACTIVE_BASE: usize = 1;
const BUCKET_TIMESHARE_BASE: usize = BUCKET_INTERACTIVE_BASE + INTERACTIVE_BUCKETS;
const BUCKET_IDLE: usize = BUCKET_TIMESHARE_BASE + TIMESHARE_BUCKETS;

static SCHED_TICKS: AtomicUsize = AtomicUsize::new(0);

/// An instance of a scheduler. Each CPU has one instance running to coordinate task management.
#[derive(Debug)]
pub struct Scheduler {
    /// The currently running task on this scheduler instance. Use [`Self::get_current`] instead.
    pub(crate) current: AtomicPtr<Task>,
    pub(crate) idle_task: AtomicPtr<Task>,
    pub(crate) preempt_level: usize,
    owner: AtomicU32,
    reschedule_pending: AtomicBool,
    load: AtomicUsize,
    run_queue: SpinMutex<RunQueue>,
}

struct RunQueue {
    kernel: VecDeque<Arc<Task>>,
    interactive: [VecDeque<Arc<Task>>; INTERACTIVE_BUCKETS],
    timeshare: [VecDeque<Arc<Task>>; TIMESHARE_BUCKETS],
    idle: VecDeque<Arc<Task>>,
    timeshare_insert: usize,
    timeshare_dequeue: usize,
}

#[derive(Clone, Copy)]
enum QueueClass {
    Kernel,
    Interactive(usize),
    Timeshare(usize),
    Idle,
}

impl RunQueue {
    const fn new() -> Self {
        Self {
            kernel: VecDeque::new(),
            interactive: [const { VecDeque::new() }; INTERACTIVE_BUCKETS],
            timeshare: [const { VecDeque::new() }; TIMESHARE_BUCKETS],
            idle: VecDeque::new(),
            timeshare_insert: 0,
            timeshare_dequeue: 0,
        }
    }

    fn push(&mut self, task: Arc<Task>, class: QueueClass) {
        let bucket = match class {
            QueueClass::Kernel => {
                self.kernel.push_back(task);
                BUCKET_KERNEL
            }
            QueueClass::Interactive(score) => {
                let index = score.min(INTERACTIVE_BUCKETS - 1);
                self.interactive[index].push_back(task);
                BUCKET_INTERACTIVE_BASE + index
            }
            QueueClass::Timeshare(priority) => {
                let mut index = (priority.min(TIMESHARE_BUCKETS - 1) + self.timeshare_insert)
                    % TIMESHARE_BUCKETS;
                if self.timeshare_dequeue != self.timeshare_insert
                    && index == self.timeshare_dequeue
                {
                    index = (index + TIMESHARE_BUCKETS - 1) % TIMESHARE_BUCKETS;
                }
                self.timeshare[index].push_back(task);
                BUCKET_TIMESHARE_BASE + index
            }
            QueueClass::Idle => {
                self.idle.push_back(task);
                BUCKET_IDLE
            }
        };

        let queued = self.bucket_back(bucket);
        if let Some(task) = queued {
            task.queued_bucket.store(bucket, Ordering::Release);
        }
    }

    fn bucket_back(&self, bucket: usize) -> Option<&Arc<Task>> {
        match bucket {
            BUCKET_KERNEL => self.kernel.back(),
            BUCKET_IDLE => self.idle.back(),
            BUCKET_INTERACTIVE_BASE..BUCKET_TIMESHARE_BASE => self
                .interactive
                .get(bucket - BUCKET_INTERACTIVE_BASE)
                .and_then(VecDeque::back),
            BUCKET_TIMESHARE_BASE..BUCKET_IDLE => self
                .timeshare
                .get(bucket - BUCKET_TIMESHARE_BASE)
                .and_then(VecDeque::back),
            _ => None,
        }
    }

    fn pop_next(&mut self) -> Option<Arc<Task>> {
        if let Some(task) = self.kernel.pop_front() {
            return Some(task);
        }

        for queue in &mut self.interactive {
            if let Some(task) = queue.pop_front() {
                return Some(task);
            }
        }

        for offset in 0..TIMESHARE_BUCKETS {
            let index = (self.timeshare_dequeue + offset) % TIMESHARE_BUCKETS;
            if let Some(task) = self.timeshare[index].pop_front() {
                if self.timeshare[index].is_empty() && index == self.timeshare_dequeue {
                    self.advance_timeshare_dequeue(true);
                }
                return Some(task);
            }
        }

        self.idle.pop_front()
    }

    fn take_stealable(&mut self) -> Option<Arc<Task>> {
        for offset in (0..TIMESHARE_BUCKETS).rev() {
            let index = (self.timeshare_dequeue + offset) % TIMESHARE_BUCKETS;
            if let Some(task) = Self::take_from_back_if(&mut self.timeshare[index]) {
                return Some(task);
            }
        }

        for queue in self.interactive.iter_mut().rev() {
            if let Some(task) = Self::take_from_back_if(queue) {
                return Some(task);
            }
        }

        Self::take_from_back_if(&mut self.kernel)
    }

    fn take_from_back_if(queue: &mut VecDeque<Arc<Task>>) -> Option<Arc<Task>> {
        let index = queue
            .iter()
            .rposition(|task| task.migration_enabled.load(Ordering::Acquire))?;
        queue.remove(index)
    }

    fn advance_timeshare(&mut self, ticks: usize) {
        if self.timeshare_insert == self.timeshare_dequeue {
            self.timeshare_insert = (self.timeshare_insert + ticks) % TIMESHARE_BUCKETS;
            self.advance_timeshare_dequeue(false);
        }
    }

    fn advance_timeshare_dequeue(&mut self, mut current_known_empty: bool) {
        while self.timeshare_dequeue != self.timeshare_insert {
            if current_known_empty {
                current_known_empty = false;
            } else if !self.timeshare[self.timeshare_dequeue].is_empty() {
                break;
            }

            self.timeshare_dequeue = (self.timeshare_dequeue + 1) % TIMESHARE_BUCKETS;
        }
    }
}

impl Scheduler {
    pub(crate) const fn new_for_cpu(owner: u32) -> Self {
        return Self {
            current: AtomicPtr::new(null_mut()),
            idle_task: AtomicPtr::new(null_mut()),
            preempt_level: 0,
            owner: AtomicU32::new(owner),
            reschedule_pending: AtomicBool::new(false),
            load: AtomicUsize::new(0),
            run_queue: SpinMutex::new(RunQueue::new()),
        };
    }

    /// Adds a task to a run queue.
    pub fn add_task(&self, task: Arc<Task>) {
        let cpu = self.owner_cpu();
        let now = SCHED_TICKS.load(Ordering::Acquire);

        let was_waiting = {
            let mut state = task.state.lock();
            match *state {
                // Already terminating, do not resurrect.
                State::Dead | State::Dying => return,
                State::Running => {
                    task.wake_pending.store(true, Ordering::Release);
                    return;
                }
                // Either already on a queue (Ready) or being woken from sleep (Waiting).
                State::Ready => false,
                State::Waiting => {
                    *state = State::Ready;
                    true
                }
            }
        };

        // Avoid double-enqueueing the same task.
        if task
            .queued
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        if was_waiting {
            let sleep_started = task.sleep_started_tick.swap(0, Ordering::AcqRel);
            if sleep_started != 0 {
                let slept = now.saturating_sub(sleep_started);
                task.sched_sleeptime.fetch_add(slept, Ordering::AcqRel);
                Self::decay_interactivity(&task);
            }
            task.sched_slice.store(0, Ordering::Release);
        }

        {
            let mut queue = self.run_queue.lock();
            self.account_load(cpu, &task);
            Self::enqueue_locked(&mut queue, task.clone());
        }

        Self::notify_cpu(cpu, &task);
    }

    fn owner_cpu(&self) -> &'static CpuData {
        if let Some(cpu) = CpuData::get_for(self.owner.load(Ordering::Acquire)) {
            return cpu;
        }

        for cpu in CpuData::iter() {
            if addr_eq(&cpu.scheduler as *const Scheduler, self as *const Scheduler) {
                self.owner.store(cpu.id, Ordering::Release);
                return cpu;
            }
        }

        CpuData::get()
    }

    fn enqueue(&self, task: Arc<Task>) {
        let mut queue = self.run_queue.lock();
        Self::enqueue_locked(&mut queue, task);
    }

    fn enqueue_locked(queue: &mut RunQueue, task: Arc<Task>) {
        let (class, priority) = Self::classify(&task);
        task.dynamic_priority.store(priority, Ordering::Release);
        queue.push(task, class);
    }

    fn finish_requeue_current(&self, cpu: &CpuData, task: Arc<Task>) {
        let owner = task.sched_cpu.load(Ordering::Acquire);
        if owner != cpu.id {
            if let Some(owner_cpu) = CpuData::get_for(owner) {
                Self::transfer_load(owner_cpu, cpu, &task);
            } else {
                self.account_load(cpu, &task);
            }
        }

        self.enqueue(task);
    }

    fn choose_target_cpu(task: &Task, honor_affinity: bool) -> &'static CpuData {
        let current_cpu = CpuData::get();
        let now = SCHED_TICKS.load(Ordering::Acquire);
        let assigned_cpu_id = task.sched_cpu.load(Ordering::Acquire);
        let last_cpu_id = if assigned_cpu_id != NO_CPU {
            assigned_cpu_id
        } else {
            task.last_cpu.load(Ordering::Acquire)
        };

        if task.load_counted.load(Ordering::Acquire) {
            if let Some(owner_cpu) = CpuData::get_for(assigned_cpu_id) {
                if owner_cpu.online.load(Ordering::Acquire) {
                    return owner_cpu;
                }
            }
        }

        if task.queued.load(Ordering::Acquire) {
            if let Some(queued_cpu) = CpuData::get_for(assigned_cpu_id) {
                if queued_cpu.online.load(Ordering::Acquire) {
                    return queued_cpu;
                }
            }
        }

        if honor_affinity {
            if let Some(last_cpu) = CpuData::get_for(last_cpu_id) {
                let last_run = task.last_run_tick.load(Ordering::Acquire);
                if last_cpu.online.load(Ordering::Acquire)
                    && now.saturating_sub(last_run) <= AFFINITY_TICKS
                {
                    return last_cpu;
                }
            }
        }

        let mut min_load = usize::MAX;
        let mut least_loaded_cpu = current_cpu;

        for cpu_data in CpuData::iter() {
            if !cpu_data.online.load(Ordering::Acquire) {
                continue;
            }

            let load = cpu_data.scheduler.load.load(Ordering::Acquire);
            if load < min_load
                || (load == min_load
                    && (cpu_data.id == current_cpu.id || cpu_data.id == last_cpu_id))
            {
                min_load = load;
                least_loaded_cpu = cpu_data;
            }
        }

        least_loaded_cpu
    }

    /// Adds a task to the run queue of the CPU with the lowest load.
    /// This is used for new process creation to balance load across CPUs.
    pub fn add_task_to_best_cpu(task: Arc<Task>) {
        let least_loaded_cpu = Self::choose_target_cpu(&task, false);
        least_loaded_cpu.scheduler.add_task(task);
    }

    /// Wakes a task on the CPU it was last assigned to.
    /// Falls back to the local CPU if it has never run or the recorded CPU is offline.
    pub fn wake_task(task: Arc<Task>) {
        let target_cpu = Self::choose_target_cpu(&task, true);
        target_cpu.scheduler.add_task(task);
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
        let cpu = self.owner_cpu();
        loop {
            let task = self.run_queue.lock().pop_next();
            let Some(task) = task else {
                return self.try_steal();
            };

            task.queued.store(false, Ordering::Release);
            task.queued_bucket.store(NO_BUCKET, Ordering::Release);

            {
                let mut state = task.state.lock();
                if *state == State::Ready {
                    *state = State::Running;
                    task.wake_pending.store(false, Ordering::Release);
                    task.migration_enabled.store(true, Ordering::Release);
                    drop(state);
                    return Some(task);
                }
                if *state == State::Running {
                    continue;
                }
            }

            task.wake_pending.store(false, Ordering::Release);
            self.unaccount_load(cpu, &task);
        }
    }

    /// Attempts to pull one ready task from the busiest remote CPU's run queue.
    /// Returns the stolen task, transferred to the caller's CPU.
    fn try_steal(&self) -> Option<Arc<Task>> {
        let local_cpu = self.owner_cpu();
        let mut victim: Option<&'static CpuData> = None;
        let mut victim_load = local_cpu.scheduler.load.load(Ordering::Acquire) + STEAL_THRESHOLD;

        for cpu in CpuData::iter() {
            if cpu.id == local_cpu.id || !cpu.online.load(Ordering::Acquire) {
                continue;
            }

            let load = cpu.scheduler.load.load(Ordering::Acquire);
            if load > victim_load {
                victim_load = load;
                victim = Some(cpu);
            }
        }

        let victim = victim?;
        let task = {
            let mut queue = victim.scheduler.run_queue.lock();
            queue.take_stealable()
        }?;
        task.queued.store(false, Ordering::Release);
        task.queued_bucket.store(NO_BUCKET, Ordering::Release);

        {
            let mut state = task.state.lock();
            if *state != State::Ready {
                if *state != State::Running {
                    drop(state);
                    task.wake_pending.store(false, Ordering::Release);
                    victim.scheduler.unaccount_load(victim, &task);
                }
                return None;
            }
            *state = State::Running;
        }

        task.wake_pending.store(false, Ordering::Release);
        task.migration_enabled.store(true, Ordering::Release);
        Self::transfer_load(victim, local_cpu, &task);
        Some(task)
    }

    /// Puts the current task back to the run queue and reschedules.
    pub fn reschedule(&self) {
        let lock = IrqLock::lock();
        let from = self.current.load(Ordering::Acquire);

        if from != self.idle_task.load(Ordering::Acquire) {
            let cpu = self.owner_cpu();
            let arc = unsafe {
                let task = Arc::from_raw(from);
                let result = task.clone();
                mem::forget(task);
                result
            };
            let queued = {
                let mut state = arc.state.lock();
                if matches!(*state, State::Dead | State::Dying) {
                    drop(state);
                    self.unaccount_load(cpu, &arc);
                    false
                } else {
                    *state = State::Ready;
                    arc.migration_enabled.store(false, Ordering::Release);
                    arc.queued
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                }
            };
            if queued {
                self.finish_requeue_current(cpu, arc);
            }
        }

        self.do_reschedule(lock);
    }

    /// Reschedules without adding the current task back to the run queue.
    pub fn do_yield(&self) {
        let lock = IrqLock::lock();
        let current_ptr = self.current.load(Ordering::Acquire);
        if current_ptr != self.idle_task.load(Ordering::Acquire) {
            // SAFETY: `current` is alive for as long as the scheduler holds it.
            let current = unsafe { &*current_ptr };
            let mut state = current.state.lock();
            if matches!(*state, State::Dead | State::Dying) {
                drop(state);
                self.unaccount_load(self.owner_cpu(), current);
            } else {
                if current.wake_pending.swap(false, Ordering::AcqRel) {
                    // A wake landed while we were running.
                    // Stay runnable and re-enqueue ourselves before yielding the CPU.
                    *state = State::Ready;
                    current.migration_enabled.store(false, Ordering::Release);
                    let queued = current
                        .queued
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok();
                    drop(state);
                    let arc = unsafe {
                        let task = Arc::from_raw(current_ptr);
                        let result = task.clone();
                        mem::forget(task);
                        result
                    };
                    let cpu = self.owner_cpu();
                    if queued {
                        self.finish_requeue_current(cpu, arc);
                    }
                } else {
                    *state = State::Waiting;
                    current
                        .sleep_started_tick
                        .store(SCHED_TICKS.load(Ordering::Acquire), Ordering::Release);
                    let cpu = self.owner_cpu();
                    self.unaccount_load(cpu, current);
                    drop(state);
                }
            }
        }
        self.do_reschedule(lock);
    }

    /// Handles a scheduler timer tick. Returns true if the current CPU should reschedule.
    pub fn tick(&self) -> bool {
        let tick = SCHED_TICKS.fetch_add(1, Ordering::AcqRel) + 1;
        let pending_reschedule = self.reschedule_pending.load(Ordering::Acquire);

        {
            let mut queue = self.run_queue.lock();
            queue.advance_timeshare(1);
        }

        let current_ptr = self.current.load(Ordering::Acquire);
        if current_ptr.is_null() || current_ptr == self.idle_task.load(Ordering::Acquire) {
            return pending_reschedule || self.load.load(Ordering::Acquire) > 0;
        }

        let current = unsafe { &*current_ptr };
        current.sched_runtime.fetch_add(1, Ordering::AcqRel);
        Self::decay_interactivity(current);
        let (_, priority) = Self::classify(current);
        current.dynamic_priority.store(priority, Ordering::Release);

        let slice = current.sched_slice.fetch_add(1, Ordering::AcqRel) + 1;
        if slice >= self.slice_ticks() {
            current.sched_slice.store(0, Ordering::Release);
            current.last_run_tick.store(tick, Ordering::Release);
            return true;
        }

        pending_reschedule
    }

    /// Requests a reschedule on this CPU, respecting the architecture preemption counter.
    pub fn request_reschedule(&self) {
        if self.current.load(Ordering::Acquire).is_null() {
            return;
        }

        unsafe { arch::sched::preempt_disable() };
        let should_reschedule = unsafe { arch::sched::preempt_enable() };
        if should_reschedule {
            self.reschedule_pending.store(false, Ordering::Release);
            self.reschedule();
        }
    }

    /// Handles a remote reschedule IPI.
    /// The IPI should make an idle CPU pick up work, but non-idle kernel contexts defer the actual switch to the
    /// timer/preemption path instead of being context-switched mid-critical section.
    pub fn handle_remote_reschedule(&self, from_user: bool) {
        let current = self.current.load(Ordering::Acquire);
        if current.is_null() {
            return;
        }

        if from_user || current == self.idle_task.load(Ordering::Acquire) {
            self.request_reschedule();
        }
    }

    pub fn has_reschedule_pending(&self) -> bool {
        self.reschedule_pending.load(Ordering::Acquire)
    }

    fn notify_cpu(cpu: &CpuData, task: &Task) {
        if !Self::should_preempt_current(cpu, task) {
            return;
        }

        if cpu.id == CpuData::get().id {
            cpu.scheduler
                .reschedule_pending
                .store(true, Ordering::Release);
            return;
        }

        if cpu
            .scheduler
            .reschedule_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe { arch::sched::remote_reschedule(cpu.id) };
        }
    }

    fn should_preempt_current(cpu: &CpuData, task: &Task) -> bool {
        let current = cpu.scheduler.current.load(Ordering::Acquire);
        if current.is_null() || current == cpu.scheduler.idle_task.load(Ordering::Acquire) {
            return true;
        }

        let new_priority = task.dynamic_priority.load(Ordering::Acquire);
        let current_priority = unsafe { (*current).dynamic_priority.load(Ordering::Acquire) };

        new_priority < current_priority
    }

    /// Runs the scheduler.
    fn do_reschedule(&self, irq_guard: IrqGuard) {
        let from = self.current.load(Ordering::Acquire);
        let idle = self.idle_task.load(Ordering::Acquire);
        let (to, to_owned) = self
            .next()
            .map(|task| (Arc::into_raw(task) as *mut _, true))
            .unwrap_or((idle, false));

        if from == to {
            if to_owned {
                _ = unsafe { Arc::from_raw(to) };
            }
            return;
        }

        let cpu = CPU_DATA.get();
        if to != idle {
            unsafe {
                (*to).last_cpu.store(cpu.id, Ordering::Release);
                (*to).sched_cpu.store(cpu.id, Ordering::Release);
                (*to)
                    .last_run_tick
                    .store(SCHED_TICKS.load(Ordering::Acquire), Ordering::Release);
                self.account_load(cpu, &*to);
            }
        }
        self.current.store(to, Ordering::Relaxed);

        unsafe {
            // If we are switching between address spaces, we need to update the page table.
            (*(*to).address_space.raw_inner()).table.set_active();

            let cpu = CPU_DATA.get();

            {
                // Save the current user stack pointer to the old task.
                (*from)
                    .user_stack
                    .store(cpu.user_stack.load(Ordering::Acquire), Ordering::Release);

                // Get the kernel and user stack pointers from the new task and write them to the per-CPU data.
                cpu.kernel_stack.store(
                    (*to).kernel_entry_stack.load(Ordering::Acquire),
                    Ordering::Release,
                );
                cpu.user_stack
                    .store((*to).user_stack.load(Ordering::Acquire), Ordering::Release);
            }

            let previous = arch::sched::switch(from, to);
            Self::post_switch(previous, irq_guard);
        }
    }

    /// Runs after a low-level context switch, once the CPU is executing on the new task's kernel stack.
    pub(crate) fn post_switch(previous: *mut Task, irq_guard: IrqGuard) {
        let idle = CPU_DATA.get().scheduler.idle_task.load(Ordering::Acquire);
        if !previous.is_null() && previous != idle {
            _ = unsafe { Arc::from_raw(previous) };
        }
        drop(irq_guard);
    }

    /// Kills the currently running task.
    pub fn kill_current() -> ! {
        let task = Scheduler::get_current();
        *task.state.lock() = State::Dead;
        drop(task);
        CPU_DATA.get().scheduler.do_yield();
        unreachable!("The scheduler did not kill this task");
    }

    pub(crate) fn set_task(&self, task: Arc<Task>) {
        let new_ptr = Arc::into_raw(task);
        let old_ptr = self.current.swap(new_ptr as *mut _, Ordering::AcqRel);
        if !old_ptr.is_null() {
            _ = unsafe { Arc::from_raw(old_ptr) }; // Arc is dropped here.
        }
    }

    fn load(&self) -> usize {
        self.load.load(Ordering::Acquire)
    }

    fn account_load(&self, cpu: &CpuData, task: &Task) {
        if task
            .load_counted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let old_cpu = task.sched_cpu.swap(cpu.id, Ordering::AcqRel);
            debug_assert!(old_cpu == NO_CPU || old_cpu == cpu.id);
            cpu.scheduler.load.fetch_add(1, Ordering::AcqRel);
        } else {
            let owner = task.sched_cpu.load(Ordering::Acquire);
            debug_assert!(owner == cpu.id);
        }
    }

    fn unaccount_load(&self, cpu: &CpuData, task: &Task) {
        if task
            .load_counted
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let owner = task.sched_cpu.swap(NO_CPU, Ordering::AcqRel);
            let load_cpu = CpuData::get_for(owner).unwrap_or(cpu);
            debug_assert!(owner == cpu.id);

            let old_load = load_cpu.scheduler.load.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(old_load > 0);
        }
    }

    fn transfer_load(from: &CpuData, to: &CpuData, task: &Task) {
        if from.id == to.id {
            task.sched_cpu.store(to.id, Ordering::Release);
            return;
        }

        if task.load_counted.load(Ordering::Acquire) {
            from.scheduler.load.fetch_sub(1, Ordering::AcqRel);
            to.scheduler.load.fetch_add(1, Ordering::AcqRel);
            task.sched_cpu.store(to.id, Ordering::Release);
        } else {
            to.scheduler.account_load(to, task);
        }
    }

    fn slice_ticks(&self) -> usize {
        let load = self.load().saturating_sub(1);
        if load >= BASE_SLICE_TICKS {
            MIN_SLICE_TICKS
        } else if load <= 1 {
            BASE_SLICE_TICKS
        } else {
            (BASE_SLICE_TICKS / load).max(MIN_SLICE_TICKS)
        }
    }

    fn classify(task: &Task) -> (QueueClass, usize) {
        if !task.is_user() {
            return (QueueClass::Kernel, BUCKET_KERNEL);
        }

        let score = Self::interact_score(task);
        if score < INTERACT_THRESH {
            let priority = BUCKET_INTERACTIVE_BASE + score;
            return (QueueClass::Interactive(score), priority);
        }

        let runtime = task.sched_runtime.load(Ordering::Acquire);
        let sleeptime = task.sched_sleeptime.load(Ordering::Acquire);
        let total = runtime.saturating_add(sleeptime).max(1);
        let cpu_bias = runtime.saturating_mul(TIMESHARE_BUCKETS - 1) / total;
        let priority = BUCKET_TIMESHARE_BASE + cpu_bias;
        (QueueClass::Timeshare(cpu_bias), priority)
    }

    fn interact_score(task: &Task) -> usize {
        let runtime = task.sched_runtime.load(Ordering::Acquire);
        let sleeptime = task.sched_sleeptime.load(Ordering::Acquire);

        if runtime > sleeptime {
            let div = (runtime / (INTERACT_MAX / 2)).max(1);
            return (INTERACT_MAX / 2)
                + ((INTERACT_MAX / 2) - (sleeptime / div).min(INTERACT_MAX / 2));
        }

        if sleeptime > runtime {
            let div = (sleeptime / (INTERACT_MAX / 2)).max(1);
            return (runtime / div).min(INTERACT_MAX);
        }

        if runtime != 0 { INTERACT_MAX / 2 } else { 0 }
    }

    fn decay_interactivity(task: &Task) {
        let runtime = task.sched_runtime.load(Ordering::Acquire);
        let sleeptime = task.sched_sleeptime.load(Ordering::Acquire);
        if runtime.saturating_add(sleeptime) <= SLEEP_RUN_MAX {
            return;
        }

        task.sched_runtime.store(runtime / 2, Ordering::Release);
        task.sched_sleeptime.store(sleeptime / 2, Ordering::Release);
    }
}

/// Generic task entry point. This is to be called by an implementing [`crate::arch::sched::init_task`].
pub extern "C" fn task_entry(entry: extern "C" fn(usize, usize), arg1: usize, arg2: usize) -> ! {
    (entry)(arg1, arg2);

    // The task function is over, kill the task.
    Scheduler::kill_current();
}

/// Generic first-run task entry point.
/// Arch switch code jumps here for tasks that have never returned from [`arch::sched::switch`] before.
pub extern "C" fn task_entry_after_switch(
    previous: *mut Task,
    entry: extern "C" fn(usize, usize),
    arg1: usize,
    arg2: usize,
) -> ! {
    let irq_guard = unsafe { IrqGuard::new_fake() };
    Scheduler::post_switch(previous, irq_guard);
    task_entry(entry, arg1, arg2)
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
