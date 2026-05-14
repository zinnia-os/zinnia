#[cfg(feature = "track_spinlock_callers")]
use core::{cell::UnsafeCell, panic::Location};
use core::{
    hint,
    sync::atomic::{AtomicU32, Ordering},
};

/// A ticket spinlock without a specific resource connected to it.
pub struct SpinLock {
    next: AtomicU32,
    owner: AtomicU32,
    #[cfg(feature = "track_spinlock_callers")]
    acquired_at: UnsafeCell<Option<&'static Location<'static>>>,
}

impl SpinLock {
    pub const fn new() -> Self {
        Self {
            next: AtomicU32::new(0),
            owner: AtomicU32::new(0),
            #[cfg(feature = "track_spinlock_callers")]
            acquired_at: UnsafeCell::new(None),
        }
    }

    #[inline(always)]
    #[track_caller]
    pub fn lock(&self) {
        let my = self.next.fetch_add(1, Ordering::Relaxed);
        while self.owner.load(Ordering::Acquire) != my {
            hint::spin_loop();
        }
        #[cfg(feature = "track_spinlock_callers")]
        {
            unsafe {
                *self.acquired_at.get() = Some(Location::caller());
            }
        }
    }

    #[inline(always)]
    pub fn unlock(&self) {
        #[cfg(feature = "track_spinlock_callers")]
        {
            unsafe {
                *self.acquired_at.get() = None;
            }
        }
        let val = self.owner.load(Ordering::Relaxed);
        self.owner.store(val.wrapping_add(1), Ordering::Release);
    }
}

unsafe impl Send for SpinLock {}
unsafe impl Sync for SpinLock {}
