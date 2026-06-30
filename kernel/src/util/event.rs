use crate::{percpu::CpuData, process::task::Task, sched::Scheduler, util::mutex::spin::SpinMutex};
use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
};
use intrusive_collections::{LinkedList, LinkedListAtomicLink, UnsafeRef, intrusive_adapter};

pub struct Waiter {
    waiters_link: LinkedListAtomicLink,
    task: Arc<Task>,
    woken: AtomicBool,
}

intrusive_adapter!(WaitersLinkAdapter = UnsafeRef<Waiter>: Waiter { waiters_link => LinkedListAtomicLink });

struct Observer {
    link: LinkedListAtomicLink,
    target: Weak<Event>,
}

intrusive_adapter!(ObserverAdapter = UnsafeRef<Observer>: Observer { link => LinkedListAtomicLink });

#[derive(Debug)]
pub struct Event {
    waiters: SpinMutex<LinkedList<WaitersLinkAdapter>>,
    observers: SpinMutex<LinkedList<ObserverAdapter>>,
    forwarding: AtomicBool,
}

impl Event {
    pub fn new() -> Self {
        Self {
            waiters: SpinMutex::new(LinkedList::new(WaitersLinkAdapter::NEW)),
            observers: SpinMutex::new(LinkedList::new(ObserverAdapter::NEW)),
            forwarding: AtomicBool::new(false),
        }
    }

    pub fn guard(&self) -> EventGuard<'_> {
        let waiter = Box::pin(Waiter {
            waiters_link: LinkedListAtomicLink::new(),
            task: Scheduler::get_current(),
            woken: AtomicBool::new(false),
        });

        let mut waiters = self.waiters.lock();

        // The waiter is pinned and will remain valid for the lifetime
        // of the EventGuard. The list holds a non-owning UnsafeRef.
        waiters.push_back(unsafe { UnsafeRef::from_raw(&*waiter as *const Waiter) });

        EventGuard {
            parent: self,
            waiter,
        }
    }

    /// Registers a waiter only if `should_wait` returns true.
    pub fn guard_if(&self, should_wait: impl FnOnce() -> bool) -> Option<EventGuard<'_>> {
        let waiter = Box::pin(Waiter {
            waiters_link: LinkedListAtomicLink::new(),
            task: Scheduler::get_current(),
            woken: AtomicBool::new(false),
        });

        let mut waiters = self.waiters.lock();

        if !should_wait() {
            return None;
        }

        waiters.push_back(unsafe { UnsafeRef::from_raw(&*waiter as *const Waiter) });

        Some(EventGuard {
            parent: self,
            waiter,
        })
    }

    /// Wakes up to `count` waiters, returning the number actually woken.
    #[track_caller]
    pub fn wake_n(&self, count: usize) -> usize {
        let mut woke = 0;
        {
            let mut waiters = self.waiters.lock();
            while woke < count {
                let Some(waiter) = waiters.pop_front() else {
                    break;
                };
                waiter.woken.store(true, Ordering::Release);
                Scheduler::wake_task(waiter.task.clone());
                woke += 1;
            }
        }
        self.forward();
        woke
    }

    #[track_caller]
    pub fn wake_all(&self) -> usize {
        let mut woke = 0;
        {
            let mut waiters = self.waiters.lock();
            for waiter in waiters.iter() {
                waiter.woken.store(true, Ordering::Release);
                Scheduler::wake_task(waiter.task.clone());
                woke += 1;
            }
            waiters.clear();
        }
        self.forward();
        woke
    }

    /// Registers `target` so a wake of this event also wakes it.
    pub fn add_observer(&self, target: Weak<Event>) -> ObserverHandle {
        let observer = Box::pin(Observer {
            link: LinkedListAtomicLink::new(),
            target,
        });
        self.observers
            .lock()
            .push_back(unsafe { UnsafeRef::from_raw(&*observer as *const Observer) });
        ObserverHandle {
            source: self,
            observer,
        }
    }

    /// Forwards a wake to live observers without holding two event locks at once.
    fn forward(&self) {
        if self.forwarding.swap(true, Ordering::AcqRel) {
            return;
        }
        let targets: Vec<Arc<Event>> = self
            .observers
            .lock()
            .iter()
            .filter_map(|o| o.target.upgrade())
            .collect();
        for target in targets {
            target.wake_all();
        }
        self.forwarding.store(false, Ordering::Release);
    }
}

pub struct EventGuard<'n> {
    parent: &'n Event,
    /// Pinned, guard-owned Waiter. The event's list holds a non-owning
    /// UnsafeRef to this, so it must outlive its list membership.
    waiter: Pin<Box<Waiter>>,
}

// Safety: The waiter is heap-allocated and pinned; the list reference is
// protected by a SpinMutex that we hold during Drop.
unsafe impl Send for EventGuard<'_> {}
unsafe impl Sync for EventGuard<'_> {}

impl<'n> EventGuard<'n> {
    #[track_caller]
    pub fn wait(&self) {
        if self.waiter.woken.load(Ordering::Acquire) {
            return;
        }
        CpuData::get().scheduler.do_yield();
    }
}

impl<'n> Drop for EventGuard<'n> {
    fn drop(&mut self) {
        let mut waiters = self.parent.waiters.lock();

        if self.waiter.waiters_link.is_linked() {
            let mut cursor = unsafe { waiters.cursor_mut_from_ptr(&*self.waiter as *const Waiter) };
            cursor.remove();
        }
    }
}

pub struct ObserverHandle {
    source: *const Event,
    observer: Pin<Box<Observer>>,
}

// Safety: like EventGuard, the observer is pinned and the list reference is protected by the
// source's SpinMutex, which we hold during Drop.
unsafe impl Send for ObserverHandle {}
unsafe impl Sync for ObserverHandle {}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        let mut observers = unsafe { &*self.source }.observers.lock();
        if self.observer.link.is_linked() {
            let mut cursor =
                unsafe { observers.cursor_mut_from_ptr(&*self.observer as *const Observer) };
            cursor.remove();
        }
    }
}
