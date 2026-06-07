use crate::{
    arch,
    irq::lock::IrqLock,
    memory::{
        VirtAddr,
        virt::{KERNEL_PAGE_TABLE, mmu::PageTable},
    },
    percpu::CpuData,
    process::task::Task,
    sched::Scheduler,
};
use alloc::{boxed::Box, sync::Arc};
use core::{
    future::Future,
    pin::Pin,
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering},
    task::{Context, Poll},
};
use intrusive_collections::{LinkedList, LinkedListAtomicLink, UnsafeRef, intrusive_adapter};

/// Binding id for the kernel/global address space.
pub const GLOBAL_BINDING_ID: i32 = -1;
/// Binding id for the single non-PCID user binding.
pub const USER_BINDING_ID: i32 = 0;

const FULL_FLUSH_PAGES: usize = 64;

pub struct ShootdownNode {
    link: LinkedListAtomicLink,
    address: usize,
    size: usize,
    sequence: u64,
    initiator_cpu: u32,
    bindings_to_shoot: AtomicUsize,
    completed: AtomicBool,
    waiter: AtomicPtr<Task>,
}

intrusive_adapter!(
    ShootdownNodeAdapter = UnsafeRef<ShootdownNode>: ShootdownNode {
        link => LinkedListAtomicLink
    }
);

impl ShootdownNode {
    fn new(address: usize, size: usize) -> Self {
        Self {
            link: LinkedListAtomicLink::new(),
            address,
            size,
            sequence: 0,
            initiator_cpu: u32::MAX,
            bindings_to_shoot: AtomicUsize::new(0),
            completed: AtomicBool::new(false),
            waiter: AtomicPtr::new(ptr::null_mut()),
        }
    }

    fn complete(&self) {
        let waiter = self.waiter.swap(ptr::null_mut(), Ordering::AcqRel);
        if waiter.is_null() {
            self.completed.store(true, Ordering::Release);
            return;
        }

        self.completed.store(true, Ordering::Release);
        unsafe { Arc::increment_strong_count(waiter) };
        Scheduler::wake_task_on_cpu(self.initiator_cpu, unsafe { Arc::from_raw(waiter) });
    }
}

pub struct ShootdownState {
    num_bindings: usize,
    shoot_sequence: u64,
    queue: LinkedList<ShootdownNodeAdapter>,
}

impl ShootdownState {
    pub const fn new() -> Self {
        Self {
            num_bindings: 0,
            shoot_sequence: 0,
            queue: LinkedList::new(ShootdownNodeAdapter::NEW),
        }
    }

    pub const fn num_bindings(&self) -> usize {
        self.num_bindings
    }
}

fn invalidate_node(is_global: bool, node: &ShootdownNode) {
    if !is_global && (node.size >> arch::virt::get_page_bits()) >= FULL_FLUSH_PAGES {
        arch::virt::flush_tlb_all();
    } else {
        let mut off = 0;
        while off < node.size {
            arch::virt::flush_tlb(VirtAddr::new(node.address + off));
            off += arch::virt::get_page_size();
        }
    }
}

#[derive(Debug)]
pub struct PageBinding {
    id: i32,
    /// When non-null, this binding owns one strong [`Arc<PageTable>`] reference
    /// retained by [`Self::retain_space`].
    bound_space: AtomicPtr<PageTable>,
    already_shot_sequence: AtomicU64,
}

impl PageBinding {
    pub const fn new(id: i32) -> Self {
        Self {
            id,
            bound_space: AtomicPtr::new(ptr::null_mut()),
            already_shot_sequence: AtomicU64::new(0),
        }
    }

    fn retain_space(space: Arc<PageTable>) -> *mut PageTable {
        Arc::into_raw(space) as *mut PageTable
    }

    unsafe fn retain_borrowed_space(space: &PageTable) -> *mut PageTable {
        let ptr = space as *const PageTable;
        unsafe { Arc::increment_strong_count(ptr) };
        ptr as *mut PageTable
    }

    unsafe fn release_space(space: *mut PageTable) {
        unsafe { drop(Arc::from_raw(space)) };
    }

    fn bound(&self) -> *mut PageTable {
        self.bound_space.load(Ordering::Acquire)
    }

    pub fn is_bound_to(&self, space: &PageTable) -> bool {
        ptr::eq(self.bound() as *const PageTable, space)
    }

    pub fn rebind(&self, space: &PageTable) {
        let _irq = IrqLock::lock();
        debug_assert_eq!(self.id, USER_BINDING_ID);

        if self.is_bound_to(space) {
            return;
        }

        let old = self.bound();
        let old_already = self.already_shot_sequence.load(Ordering::Relaxed);
        let retained = unsafe { Self::retain_borrowed_space(space) };

        let target_seq = {
            let mut state = space.shoot().lock();
            state.num_bindings += 1;
            state.shoot_sequence
        };
        self.bound_space.store(retained, Ordering::Release);
        self.already_shot_sequence
            .store(target_seq, Ordering::Relaxed);

        unsafe { arch::virt::set_page_table(space) };

        if !old.is_null() {
            self.leave(unsafe { &*old }, old_already);
            unsafe { Self::release_space(old) };
        }
    }

    pub fn release(&self) {
        let _irq = IrqLock::lock();
        debug_assert_eq!(self.id, USER_BINDING_ID);

        let old = self.bound();
        if old.is_null() {
            return;
        }

        let old_already = self.already_shot_sequence.load(Ordering::Relaxed);
        self.leave(unsafe { &*old }, old_already);
        self.bound_space.store(ptr::null_mut(), Ordering::Release);
        self.already_shot_sequence.store(0, Ordering::Relaxed);
        unsafe { Self::release_space(old) };
    }

    pub fn initial_bind_global(&self, space: Arc<PageTable>) {
        let _irq = IrqLock::lock();
        debug_assert_eq!(self.id, GLOBAL_BINDING_ID);
        debug_assert!(self.bound().is_null());

        let target_seq = {
            let mut state = space.shoot().lock();
            state.num_bindings += 1;
            state.shoot_sequence
        };
        let retained = Self::retain_space(space);
        self.bound_space.store(retained, Ordering::Release);
        self.already_shot_sequence
            .store(target_seq, Ordering::Relaxed);
    }

    fn leave(&self, space: &PageTable, old_already: u64) {
        let up_to = {
            let mut state = space.shoot().lock();
            state.num_bindings -= 1;
            state.shoot_sequence
        };
        self.drain_shootdown(space, old_already, up_to);
    }

    fn shootdown(&self) {
        let space = self.bound();
        if space.is_null() {
            return;
        }
        self.do_shootdown(unsafe { &*space });
    }

    fn do_shootdown(&self, space: &PageTable) {
        let is_global = self.id == GLOBAL_BINDING_ID;
        let this_cpu = CpuData::get().id;
        let already = self.already_shot_sequence.load(Ordering::Relaxed);
        let mut max_seq = already;

        let mut to_complete = LinkedList::new(ShootdownNodeAdapter::NEW);

        {
            let mut state = space.shoot().lock();
            let mut cursor = state.queue.back_mut();
            let Some(back) = cursor.get() else {
                return;
            };
            if back.sequence <= already {
                return;
            }

            while let Some(prev) = cursor.peek_prev().get() {
                if prev.sequence <= already {
                    break;
                }
                cursor.move_prev();
            }

            while let Some(node) = cursor.get() {
                let seq = node.sequence;
                if node.initiator_cpu != this_cpu {
                    invalidate_node(is_global, node);
                    if node.bindings_to_shoot.fetch_sub(1, Ordering::AcqRel) == 1 {
                        max_seq = max_seq.max(seq);
                        if let Some(removed) = cursor.remove() {
                            to_complete.push_back(removed);
                        }
                        continue;
                    }
                }
                max_seq = max_seq.max(seq);
                cursor.move_next();
            }

            self.already_shot_sequence.store(max_seq, Ordering::Relaxed);
        }

        drain_completions(&mut to_complete);
    }

    fn drain_shootdown(&self, space: &PageTable, after_seq: u64, up_to_seq: u64) {
        let this_cpu = CpuData::get().id;
        let mut to_complete = LinkedList::new(ShootdownNodeAdapter::NEW);

        {
            let mut state = space.shoot().lock();
            let mut cursor = state.queue.back_mut();
            let Some(back) = cursor.get() else {
                return;
            };
            if back.sequence <= after_seq {
                return;
            }

            while let Some(prev) = cursor.peek_prev().get() {
                if prev.sequence <= after_seq {
                    break;
                }
                cursor.move_prev();
            }

            while let Some(node) = cursor.get() {
                let seq = node.sequence;
                if seq > up_to_seq {
                    break;
                }
                if node.initiator_cpu != this_cpu
                    && node.bindings_to_shoot.fetch_sub(1, Ordering::AcqRel) == 1
                {
                    if let Some(removed) = cursor.remove() {
                        to_complete.push_back(removed);
                    }
                    continue;
                }
                cursor.move_next();
            }
        }

        drain_completions(&mut to_complete);
    }
}

fn drain_completions(to_complete: &mut LinkedList<ShootdownNodeAdapter>) {
    while let Some(node) = to_complete.pop_front() {
        let ptr = UnsafeRef::into_raw(node);
        unsafe { (*ptr).complete() };
    }
}

pub fn service_shootdowns() {
    let cpu = CpuData::get();
    cpu.user_binding.shootdown();
    cpu.global_binding.shootdown();
}

pub fn init_cpu() {
    CpuData::get()
        .global_binding
        .initial_bind_global(KERNEL_PAGE_TABLE.get().clone());
}

pub struct ShootdownFuture<'a> {
    space: &'a PageTable,
    node: Pin<Box<ShootdownNode>>,
    waiter: Option<Arc<Task>>,
    started: bool,
}

unsafe impl Send for ShootdownFuture<'_> {}
impl Unpin for ShootdownFuture<'_> {}

impl<'a> ShootdownFuture<'a> {
    fn new(space: &'a PageTable, address: usize, size: usize) -> Self {
        Self {
            space,
            node: Box::pin(ShootdownNode::new(address, size)),
            waiter: None,
            started: false,
        }
    }
}

impl Future for ShootdownFuture<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.node.size == 0 || self.node.completed.load(Ordering::Acquire) {
            self.waiter.take();
            return Poll::Ready(());
        }

        if !self.started {
            let task = Scheduler::get_current();
            self.node
                .waiter
                .store(Arc::as_ptr(&task) as *mut Task, Ordering::Release);
            self.waiter = Some(task);
            self.started = true;

            if submit_node(self.space, &mut self.node) {
                self.waiter.take();
                return Poll::Ready(());
            }
        }

        if self.node.completed.load(Ordering::Acquire) {
            self.waiter.take();
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

pub fn shootdown(space: &PageTable, address: usize, size: usize) -> ShootdownFuture<'_> {
    ShootdownFuture::new(space, address, size)
}

pub fn submit_shootdown(space: &PageTable, address: usize, size: usize) {
    if size == 0 {
        return;
    }

    let scheduler = &CpuData::get().scheduler;
    if scheduler.has_current() {
        debug_assert!(
            crate::arch::irq::get_irq_state(),
            "submit_shootdown blocked with interrupts disabled (spinlock held?)"
        );
        scheduler.block_on(shootdown(space, address, size));
        return;
    }

    let mut node = ShootdownNode::new(address, size);
    if submit_node(space, &mut node) {
        return;
    }

    while !node.completed.load(Ordering::Acquire) {
        service_shootdowns();
        core::hint::spin_loop();
    }
}

fn submit_node(space: &PageTable, node: &mut ShootdownNode) -> bool {
    let is_global = !space.is_user_space();
    let cpu = CpuData::get();

    unsafe { arch::sched::preempt_disable() };

    node.initiator_cpu = cpu.id;
    let mut local_bindings = 0usize;
    let need_remote = {
        let mut state = space.shoot().lock();

        if cpu.global_binding.is_bound_to(space) {
            invalidate_node(true, node);
            local_bindings += 1;
        } else if is_global {
            invalidate_node(true, node);
        }

        if cpu.user_binding.is_bound_to(space) {
            invalidate_node(false, node);
            local_bindings += 1;
        }

        let unshot = state.num_bindings.saturating_sub(local_bindings);
        if unshot == 0 {
            node.completed.store(true, Ordering::Release);
            false
        } else {
            state.shoot_sequence += 1;
            node.sequence = state.shoot_sequence;
            node.bindings_to_shoot.store(unshot, Ordering::Release);
            state
                .queue
                .push_back(unsafe { UnsafeRef::from_raw(node as *const ShootdownNode) });
            true
        }
    };

    if need_remote {
        arch::sched::broadcast_shootdown();
    }

    let reschedule = unsafe { arch::sched::preempt_enable() };
    if reschedule {
        cpu.scheduler.request_reschedule();
    }

    !need_remote
}
