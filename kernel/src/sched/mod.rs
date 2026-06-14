use crate::{
    arch::{self},
    irq::lock::{IrqGuard, IrqLock},
    percpu::{CPU_DATA, CpuData},
    posix::errno::EResult,
    process::{
        Process,
        task::{State, Task},
    },
    util::mutex::spin::SpinMutex,
};
use alloc::sync::Arc;
use core::{
    future::Future,
    mem,
    ptr::{addr_eq, null_mut},
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering},
    task::{Context, Poll},
};
use intrusive_collections::{LinkedList, LinkedListAtomicLink, UnsafeRef, intrusive_adapter};

const NO_CPU: u32 = u32::MAX;
const BASE_SLICE_TICKS: usize = 10;
const MIN_SLICE_TICKS: usize = 1;
const AFFINITY_TICKS: usize = 4;
const WAKE_AFFINE_LOAD_MARGIN: usize = 1;
const STEAL_THRESHOLD: usize = 2;
const SLEEP_RUN_MAX: usize = 512;

const INTERACT_MAX: usize = 100;
const INTERACT_THRESH: usize = 30;

const RQ_NQS: usize = 64;
const RQB_BITS: usize = usize::BITS as usize;
const RQB_LEN: usize = RQ_NQS / RQB_BITS;

const PRI_REALTIME_BASE: usize = 0;
const PRI_TIMESHARE_BASE: usize = RQ_NQS;

static SCHED_TICKS: AtomicUsize = AtomicUsize::new(0);

intrusive_adapter!(TaskRunLink = UnsafeRef<Task>: Task { run_link => LinkedListAtomicLink });
intrusive_adapter!(TaskReapLink = UnsafeRef<Task>: Task { reap_link => LinkedListAtomicLink });

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
    reap_queue: SpinMutex<LinkedList<TaskReapLink>>,
    reaper_task: AtomicPtr<Task>,
}

struct Runq {
    status: [usize; RQB_LEN],
    queues: [LinkedList<TaskRunLink>; RQ_NQS],
}

impl Runq {
    const fn new() -> Self {
        Self {
            status: [0; RQB_LEN],
            queues: [const { LinkedList::new(TaskRunLink::NEW) }; RQ_NQS],
        }
    }

    fn set_bit(&mut self, idx: usize) {
        self.status[idx / RQB_BITS] |= 1 << (idx % RQB_BITS);
    }

    fn clear_bit(&mut self, idx: usize) {
        self.status[idx / RQB_BITS] &= !(1 << (idx % RQB_BITS));
    }

    fn is_set(&self, idx: usize) -> bool {
        self.status[idx / RQB_BITS] & (1 << (idx % RQB_BITS)) != 0
    }

    fn add(&mut self, idx: usize, task: Arc<Task>) {
        self.set_bit(idx);
        self.queues[idx].push_back(unsafe { UnsafeRef::from_raw(Arc::into_raw(task)) });
    }

    fn find_bit(&self) -> Option<usize> {
        for (word, &bits) in self.status.iter().enumerate() {
            if bits != 0 {
                return Some(word * RQB_BITS + bits.trailing_zeros() as usize);
            }
        }
        None
    }

    fn find_bit_from(&self, start: usize) -> Option<usize> {
        let start_word = start / RQB_BITS;
        let masked = self.status[start_word] & (usize::MAX << (start % RQB_BITS));
        if masked != 0 {
            return Some(start_word * RQB_BITS + masked.trailing_zeros() as usize);
        }
        for offset in 1..=RQB_LEN {
            #[allow(clippy::modulo_one)]
            let word = (start_word + offset) % RQB_LEN;
            if self.status[word] != 0 {
                return Some(word * RQB_BITS + self.status[word].trailing_zeros() as usize);
            }
        }
        None
    }

    fn pop_at(&mut self, idx: usize) -> Arc<Task> {
        let node = self.queues[idx].pop_front().unwrap();
        if self.queues[idx].is_empty() {
            self.clear_bit(idx);
        }
        unsafe { Arc::from_raw(UnsafeRef::into_raw(node)) }
    }

    fn take_migratable(&mut self, idx: usize) -> Option<Arc<Task>> {
        let node = {
            let mut cursor = self.queues[idx].front_mut();
            loop {
                match cursor.get() {
                    Some(task)
                        if task.migration_enabled.load(Ordering::Acquire)
                            && !task.bound.load(Ordering::Acquire) =>
                    {
                        break cursor.remove();
                    }
                    Some(_) => cursor.move_next(),
                    None => break None,
                }
            }
        }?;
        if self.queues[idx].is_empty() {
            self.clear_bit(idx);
        }
        Some(unsafe { Arc::from_raw(UnsafeRef::into_raw(node)) })
    }

    fn steal_from(&mut self, start: usize) -> Option<Arc<Task>> {
        for offset in 0..RQ_NQS {
            let idx = (start + offset) % RQ_NQS;
            if self.is_set(idx) {
                if let Some(task) = self.take_migratable(idx) {
                    return Some(task);
                }
            }
        }
        None
    }
}

struct RunQueue {
    realtime: Runq,
    timeshare: Runq,
    idle: Runq,
    idx: usize,
    ridx: usize,
}

#[derive(Clone, Copy)]
enum RunqClass {
    Realtime(usize),
    Timeshare(usize),
    #[allow(dead_code)]
    Idle,
}

impl RunQueue {
    const fn new() -> Self {
        Self {
            realtime: Runq::new(),
            timeshare: Runq::new(),
            idle: Runq::new(),
            idx: 0,
            ridx: 0,
        }
    }

    fn push(&mut self, task: Arc<Task>, class: RunqClass) {
        match class {
            RunqClass::Realtime(idx) => self.realtime.add(idx.min(RQ_NQS - 1), task),
            RunqClass::Timeshare(base) => {
                let mut idx = (base.min(RQ_NQS - 1) + self.idx) % RQ_NQS;
                if self.ridx != self.idx && idx == self.ridx {
                    idx = (self.ridx + RQ_NQS - 1) % RQ_NQS;
                }
                self.timeshare.add(idx, task);
            }
            RunqClass::Idle => self.idle.add(0, task),
        }
    }

    fn pop_next(&mut self) -> Option<Arc<Task>> {
        if let Some(idx) = self.realtime.find_bit() {
            return Some(self.realtime.pop_at(idx));
        }
        if let Some(idx) = self.timeshare.find_bit_from(self.ridx) {
            let task = self.timeshare.pop_at(idx);
            if idx == self.ridx && !self.timeshare.is_set(idx) {
                self.ridx = (self.ridx + 1) % RQ_NQS;
            }
            return Some(task);
        }
        self.idle.find_bit().map(|idx| self.idle.pop_at(idx))
    }

    fn take_stealable(&mut self) -> Option<Arc<Task>> {
        self.realtime
            .steal_from(0)
            .or_else(|| self.timeshare.steal_from(self.ridx))
            .or_else(|| self.idle.steal_from(0))
    }

    fn advance_timeshare(&mut self) {
        if self.idx == self.ridx {
            self.idx = (self.idx + 1) % RQ_NQS;
            if !self.timeshare.is_set(self.ridx) {
                self.ridx = self.idx;
            }
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
            reap_queue: SpinMutex::new(LinkedList::new(TaskReapLink::NEW)),
            reaper_task: AtomicPtr::new(null_mut()),
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
                    if current_cpu.online.load(Ordering::Acquire) && current_cpu.id != last_cpu.id {
                        let current_load = current_cpu.scheduler.load.load(Ordering::Acquire);
                        let last_load = last_cpu.scheduler.load.load(Ordering::Acquire);
                        if current_load <= last_load.saturating_add(WAKE_AFFINE_LOAD_MARGIN) {
                            return current_cpu;
                        }
                    }
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

    /// Picks where to enqueue a woken task.
    fn wake_target(task: &Task, preferred: &'static CpuData) -> &'static CpuData {
        if task.on_cpu.load(Ordering::Acquire)
            && let Some(home) = CpuData::get_for(task.last_cpu.load(Ordering::Acquire))
            && home.online.load(Ordering::Acquire)
        {
            return home;
        }
        preferred
    }

    /// Wakes a task on the CPU it was last assigned to.
    /// Falls back to the local CPU if it has never run or the recorded CPU is offline.
    pub fn wake_task(task: Arc<Task>) {
        let target_cpu = Self::wake_target(&task, Self::choose_target_cpu(&task, true));
        target_cpu.scheduler.add_task(task);
    }

    /// Wakes a task on a specific CPU when the waiter has CPU-owned completion
    /// state, such as a remote TLB shootdown initiated from that CPU.
    pub fn wake_task_on_cpu(cpu_id: u32, task: Arc<Task>) {
        let Some(target_cpu) =
            CpuData::get_for(cpu_id).filter(|cpu| cpu.online.load(Ordering::Acquire))
        else {
            Self::wake_task(task);
            return;
        };

        let target_cpu = Self::wake_target(&task, target_cpu);
        target_cpu.scheduler.add_task(task);
    }

    /// Returns the task currently running on this CPU.
    pub fn get_current() -> Arc<Task> {
        CPU_DATA.get().scheduler.current()
    }

    pub fn current(&self) -> Arc<Task> {
        let ptr = self.current.load(Ordering::Acquire);
        debug_assert!(!ptr.is_null());

        // If we don't do this, then the Arc's refcount won't get incremented.
        let task = unsafe { Arc::from_raw(ptr) };
        let result = task.clone();
        mem::forget(task);
        result
    }

    pub(crate) fn has_current(&self) -> bool {
        !self.current.load(Ordering::Acquire).is_null()
    }

    fn next(&self) -> Option<Arc<Task>> {
        let cpu = self.owner_cpu();
        loop {
            let task = self.run_queue.lock().pop_next();
            let Some(task) = task else {
                return self.try_steal();
            };

            task.queued.store(false, Ordering::Release);

            {
                let mut state = task.state.lock();
                if *state == State::Ready {
                    *state = State::Running;
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

        // A pending wake must survive task selection and be consumed only by the task's own `do_yield`.
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
            } else if current.wake_pending.swap(false, Ordering::AcqRel) {
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
                current.migration_enabled.store(false, Ordering::Release);
                current
                    .sleep_started_tick
                    .store(SCHED_TICKS.load(Ordering::Acquire), Ordering::Release);
                let cpu = self.owner_cpu();
                self.unaccount_load(cpu, current);
                drop(state);
            }
        }
        self.do_reschedule(lock);
    }

    /// Handles a scheduler timer tick. Returns true if the current CPU should reschedule.
    pub fn tick(&self) -> bool {
        let cpu = self.owner_cpu();
        let tick = if cpu.id == 0 {
            SCHED_TICKS.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            SCHED_TICKS.load(Ordering::Acquire)
        };
        let pending_reschedule = self.reschedule_pending.load(Ordering::Acquire);

        {
            let mut queue = self.run_queue.lock();
            queue.advance_timeshare();
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
            self.reschedule_pending.store(true, Ordering::Release);
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

    fn notify_cpu(cpu: &'static CpuData, task: &Task) {
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
        self.current.store(to, Ordering::Release);

        // Claim the new task's context for this CPU.
        unsafe { (*to).on_cpu.store(true, Ordering::Release) };

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
                cpu.kernel_stack
                    .store((*to).kernel_stack.top().value(), Ordering::Release);
                cpu.user_stack
                    .store((*to).user_stack.load(Ordering::Acquire), Ordering::Release);
            }

            let previous = arch::sched::switch(from, to);
            Self::post_switch(previous, irq_guard);
        }
    }

    /// Runs after a low-level context switch, once the CPU is executing on the new task's kernel stack.
    pub(crate) fn post_switch(previous: *mut Task, irq_guard: IrqGuard) {
        if !previous.is_null() {
            unsafe { (*previous).on_cpu.store(false, Ordering::Release) };
        }

        let idle = CPU_DATA.get().scheduler.idle_task.load(Ordering::Acquire);
        if !previous.is_null() && previous != idle {
            let previous = unsafe { Arc::from_raw(previous) };
            if Arc::strong_count(&previous) == 1 {
                CPU_DATA.get().scheduler.queue_reap(previous);
            }
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

    pub(crate) fn set_reaper_task(&self, task: Arc<Task>) {
        let new_ptr = Arc::into_raw(task);
        let old_ptr = self.reaper_task.swap(new_ptr as *mut _, Ordering::AcqRel);
        if !old_ptr.is_null() {
            _ = unsafe { Arc::from_raw(old_ptr) };
        }
    }

    fn queue_reap(&self, task: Arc<Task>) {
        self.reap_queue
            .lock()
            .push_back(unsafe { UnsafeRef::from_raw(Arc::into_raw(task)) });

        let reaper = self.reaper();
        self.add_task(reaper);
    }

    fn reaper(&self) -> Arc<Task> {
        let ptr = self.reaper_task.load(Ordering::Acquire);
        debug_assert!(!ptr.is_null());

        let task = unsafe { Arc::from_raw(ptr) };
        let result = task.clone();
        mem::forget(task);
        result
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

    fn classify(task: &Task) -> (RunqClass, usize) {
        if !task.is_user() {
            return (RunqClass::Realtime(0), PRI_REALTIME_BASE);
        }

        let score = Self::interact_score(task);
        if score < INTERACT_THRESH {
            let idx = 1 + score;
            return (RunqClass::Realtime(idx), PRI_REALTIME_BASE + idx);
        }

        let runtime = task.sched_runtime.load(Ordering::Acquire);
        let sleeptime = task.sched_sleeptime.load(Ordering::Acquire);
        let total = runtime.saturating_add(sleeptime).max(1);
        let cpu_bias = runtime.saturating_mul(RQ_NQS - 1) / total;
        (
            RunqClass::Timeshare(cpu_bias),
            PRI_TIMESHARE_BASE + cpu_bias,
        )
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

    /// Blocks on an async future.
    #[track_caller]
    pub fn block_on<T, F: Future<Output = T>>(&self, future: F) -> F::Output {
        let mut future = core::pin::pin!(future);

        let task = self.current();

        loop {
            let token = task.next_block_token();
            let waker = task.waker(token);
            let mut ctx = Context::from_waker(&waker);
            match future.as_mut().poll(&mut ctx) {
                Poll::Ready(output) => {
                    return output;
                }
                Poll::Pending => self.do_yield(),
            }
        }
    }

    pub fn spawn_kernel_async<F>(future: F) -> EResult<Arc<Task>>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = Task::run_async(future)?;
        Self::add_task_to_best_cpu(task.clone());
        Ok(task)
    }
}

pub struct YieldNow {
    yielded: bool,
}

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

pub const fn yield_now() -> YieldNow {
    YieldNow { yielded: false }
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
        unsafe { crate::arch::irq::set_irq_state(false) };
        if CPU_DATA.get().scheduler.has_reschedule_pending() {
            unsafe { crate::arch::irq::set_irq_state(true) };
            CPU_DATA.get().scheduler.request_reschedule();
            continue;
        }

        crate::arch::irq::wait_for_irq();
    }
}

pub extern "C" fn dummy_fn(_: usize, _: usize) {
    unreachable!("Tried to actually run a dummy task");
}

pub extern "C" fn reaper_fn(_: usize, _: usize) {
    loop {
        loop {
            let node = CPU_DATA.get().scheduler.reap_queue.lock().pop_front();
            let Some(node) = node else {
                break;
            };
            drop(unsafe { Arc::from_raw(UnsafeRef::into_raw(node)) });
        }

        CpuData::get().scheduler.do_yield();
    }
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

    let reaper_task = Arc::new(Task::new(reaper_fn, 0, 0, Process::get_kernel(), false).unwrap());
    reaper_task.bound.store(true, Ordering::Release);
    bsp.set_reaper_task(reaper_task);

    // Create a dummy task to drop right after the first reschedule.
    let dummy = Arc::new(Task::new(dummy_fn, 0, 0, Process::get_kernel(), false).unwrap());

    // Add the main function as the first task.
    let initial_task =
        Arc::new(Task::new(crate::main, 0, 0, Process::get_kernel(), false).unwrap());
    bsp.add_task(initial_task);
    bsp.set_task(dummy);
}
