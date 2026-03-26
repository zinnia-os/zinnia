//! Safe user memory reading/writing.

use super::VirtAddr;
use crate::arch;
use alloc::{ffi::CString, vec::Vec};
use core::{marker::PhantomData, slice};

/// Provides safe access to a single structure from userland.
/// [`crate::uapi`] depends on this being the same size as a pointer.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct UserPtr<T: Sized + Copy> {
    addr: VirtAddr,
    _p: PhantomData<T>,
}

// A UserPtr is always transparent and equal in size to a regular pointer.
static_assert!(size_of::<UserPtr<()>>() == size_of::<*const ()>());

impl<T: Sized + Copy> UserPtr<T> {
    /// If `addr` is a user address, returns a new instance.
    pub fn new(addr: VirtAddr) -> Self {
        Self {
            addr,
            _p: PhantomData,
        }
    }

    pub const fn addr(&self) -> VirtAddr {
        self.addr
    }

    pub const fn is_null(&self) -> bool {
        self.addr.is_null()
    }

    /// Converts this [`UserPtr`] into another type.
    pub fn convert<R: Sized + Copy>(self) -> UserPtr<R> {
        UserPtr {
            addr: self.addr,
            _p: PhantomData,
        }
    }

    /// Creates a new pointer with an offset as a multiple of the underlying type.
    pub fn offset(self, offset: usize) -> Self {
        Self::new(self.addr + VirtAddr::new(offset))
    }

    #[must_use]
    pub fn read(&self) -> Option<T> {
        let mut buf: T = unsafe { core::mem::zeroed() };
        arch::virt::copy_from_user(
            unsafe { core::slice::from_raw_parts_mut(&raw mut buf as _, size_of::<T>()) },
            self.addr,
        )
        .then_some(buf)
    }

    #[must_use]
    pub fn write(&mut self, value: T) -> Option<()> {
        arch::virt::copy_to_user(self.addr, unsafe {
            core::slice::from_raw_parts(&raw const value as _, size_of::<T>())
        })
        .then_some(())
    }

    #[must_use]
    pub fn read_slice(&self, value: &mut [T]) -> Option<()> {
        arch::virt::copy_from_user(
            unsafe {
                slice::from_raw_parts_mut(value.as_mut_ptr() as _, value.len() * size_of::<T>())
            },
            self.addr,
        )
        .then_some(())
    }

    #[must_use]
    pub fn write_slice(&mut self, value: &[T]) -> Option<()> {
        arch::virt::copy_to_user(self.addr, unsafe {
            slice::from_raw_parts(value.as_ptr() as _, value.len() * size_of::<T>())
        })
        .then_some(())
    }
}

#[repr(transparent)]
pub struct UserCStr {
    addr: VirtAddr,
}

impl UserCStr {
    pub const fn new(addr: VirtAddr) -> Self {
        Self { addr }
    }

    pub fn as_vec(&self, max_len: usize) -> Option<Vec<u8>> {
        let len = arch::virt::cstr_len_user(self.addr, max_len)?;
        let mut buf = vec![0u8; len];
        if arch::virt::copy_from_user(&mut buf, self.addr) {
            Some(buf)
        } else {
            None
        }
    }

    pub fn as_cstring(&self, max_len: usize) -> Option<CString> {
        Some(unsafe { CString::from_vec_unchecked(self.as_vec(max_len)?) })
    }
}

#[repr(C)]
pub struct UserAccessRegion {
    pub start_ip: &'static unsafe extern "C" fn(),
    pub end_ip: &'static unsafe extern "C" fn(),
    pub fault_ip: &'static unsafe extern "C" fn(),
}

unsafe impl Sync for UserAccessRegion {}
unsafe impl Send for UserAccessRegion {}
