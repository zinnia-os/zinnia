use crate::arch;
use core::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

per_cpu!(
    static IRQ_STATE: IrqState = IrqState {
        depth: AtomicUsize::new(0),
        saved_if: AtomicBool::new(false),
    };
);

struct IrqState {
    depth: AtomicUsize,
    saved_if: AtomicBool,
}

pub struct IrqLock;

impl IrqLock {
    pub fn lock() -> IrqGuard {
        let prev = unsafe { arch::irq::set_irq_state(false) };
        let state = IRQ_STATE.get();
        if state.depth.fetch_add(1, Ordering::Relaxed) == 0 {
            state.saved_if.store(prev, Ordering::Relaxed);
        }
        IrqGuard { _p: PhantomData }
    }
}

pub struct IrqGuard {
    _p: PhantomData<*const ()>,
}

impl IrqGuard {
    /// Creates a fake IrqGuard that will re-enable interrupts on drop.
    /// # Safety
    /// Intended to be used during rescheduling, where a newly scheduled task
    /// must begin execution with interrupts enabled.
    pub unsafe fn new_fake() -> Self {
        Self { _p: PhantomData }
    }
}

impl Drop for IrqGuard {
    fn drop(&mut self) {
        let state = IRQ_STATE.get();
        if state.depth.fetch_sub(1, Ordering::Relaxed) == 1
            && state.saved_if.load(Ordering::Relaxed)
        {
            unsafe { arch::irq::set_irq_state(true) };
        }
    }
}
