use core::{
    hint,
    sync::atomic::{AtomicU32, Ordering},
};

/// A ticket spinlock without a specific resource connected to it.
pub struct SpinLock {
    next: AtomicU32,
    owner: AtomicU32,
}

impl SpinLock {
    pub const fn new() -> Self {
        Self {
            next: AtomicU32::new(0),
            owner: AtomicU32::new(0),
        }
    }

    #[inline(always)]
    pub fn lock(&mut self) {
        let my = self.next.fetch_add(1, Ordering::Relaxed);
        while self.owner.load(Ordering::Acquire) != my {
            hint::spin_loop();
        }
    }

    #[inline(always)]
    pub fn unlock(&mut self) {
        let val = self.owner.load(Ordering::Relaxed);
        self.owner.store(val.wrapping_add(1), Ordering::Release);
    }
}
