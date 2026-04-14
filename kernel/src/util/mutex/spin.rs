use core::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    ops::{Deref, DerefMut},
};

use crate::irq::lock::IrqGuard;
use crate::{irq::lock::IrqLock, util::spin::SpinLock};

/// A locking primitive for mutually exclusive accesses.
/// `T` is the type of the inner value to store.
pub struct SpinMutex<T: ?Sized> {
    inner: UnsafeCell<InnerSpinMutex<T>>,
}

/// The inner workings of a [`SpinMutex`].
struct InnerSpinMutex<T: ?Sized> {
    spin: SpinLock,
    data: T,
}

impl<T> SpinMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            inner: UnsafeCell::new(InnerSpinMutex {
                spin: SpinLock::new(),
                data,
            }),
        }
    }
}

impl<T: Default> Default for SpinMutex<T> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: ?Sized> SpinMutex<T> {
    #[track_caller]
    pub fn lock(&self) -> SpinMutexGuard<'_, T> {
        let irq_guard = IrqLock::lock();
        let inner = unsafe { &mut *self.inner.get() };
        inner.spin.lock();
        SpinMutexGuard {
            parent: self,
            _irq_guard: irq_guard,
        }
    }
}

impl<T: ?Sized> SpinMutex<T> {
    /// Returns a pointer to the contained value.
    ///
    /// # Safety
    /// The caller must ensure that the contained data isn't accessed by a different caller.
    pub unsafe fn raw_inner(&self) -> *mut T {
        let inner = unsafe { &mut *self.inner.get() };
        &mut inner.data
    }
}

impl<T> SpinMutex<T> {
    #[track_caller]
    pub fn into_inner(self) -> T {
        let _irq = IrqLock::lock();
        let inner = unsafe { &mut *self.inner.get() };
        inner.spin.lock();
        self.inner.into_inner().data
    }
}

unsafe impl<T: ?Sized + Send> Send for SpinMutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for SpinMutex<T> {}

impl<T: ?Sized> Debug for SpinMutex<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpinMutex").finish()
    }
}

/// This struct is returned by [`SpinMutex::lock`] and is used to safely control mutex locking state.
pub struct SpinMutexGuard<'m, T: 'm + ?Sized> {
    parent: &'m SpinMutex<T>,
    _irq_guard: IrqGuard,
}

impl<T: ?Sized> Deref for SpinMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.parent.inner.get()).data }
    }
}

impl<T: ?Sized> DerefMut for SpinMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.parent.inner.get()).data }
    }
}

/// A guard is only valid in the current thread and any attempt to move it out is illegal.
impl<T: ?Sized> !Send for SpinMutexGuard<'_, T> {}

/// # Safety
/// We can guarantee that types encapuslated by a [`SpinMutex`] are thread safe.
unsafe impl<T: ?Sized + Sync> Sync for SpinMutexGuard<'_, T> {}

impl<T: ?Sized + Debug> Debug for SpinMutexGuard<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self.deref(), f)
    }
}

impl<T: ?Sized> Drop for SpinMutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            (*self.parent.inner.get()).spin.unlock();
        }
    }
}
