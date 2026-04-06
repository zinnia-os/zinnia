//! Helpers for structured data accesses.

use super::{PhysAddr, VirtAddr, pmm::KernelAlloc, virt::VmFlags};
use crate::memory::virt::mmu::PageTable;
use core::{marker::PhantomData, ops::RangeInclusive};
use num_traits::{FromBytes, PrimInt, ToBytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitValue<T: PrimInt> {
    value: T,
}

fn field_mask<T: PrimInt, A: PrimInt + Into<T>>(field: Field<T, A>) -> T {
    if field.bit_width == size_of::<T>() * 8 {
        T::max_value()
    } else {
        let mask: T = A::one().into();
        (mask << field.bit_width) - A::one().into()
    }
}

impl<T: PrimInt> BitValue<T> {
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    pub const fn value(&self) -> T {
        self.value
    }

    pub fn read_field<A: PrimInt + TryFrom<T> + Into<T>>(self, field: Field<T, A>) -> BitValue<A> {
        let value = (self.value >> field.field_offset) & field_mask(field);
        BitValue::new(value.try_into().ok().unwrap())
    }

    pub fn write_field<A: PrimInt + From<A>>(self, field: Field<T, A>, value: A) -> Self
    where
        T: From<A>,
    {
        let value: T = value.into();
        BitValue::new(
            (self.value & !(field_mask(field) << field.field_offset))
                | (value << field.field_offset),
        )
    }
}

pub trait MemoryView {
    /// Reads data from a register.
    fn read_reg<T: PrimInt + FromBytes>(&self, reg: Register<T>) -> Option<BitValue<T>>
    where
        T::Bytes: Default;

    /// Writes data to a register.
    fn write_reg<T: PrimInt + ToBytes>(&mut self, reg: Register<T>, value: T) -> Option<()>
    where
        T::Bytes: Default;
}

pub trait UnsafeMemoryView {
    /// Reads data from a register.
    /// # Safety
    /// The implementation must ensure that the register access is valid.
    unsafe fn read_reg<T: PrimInt + FromBytes>(&self, reg: Register<T>) -> Option<BitValue<T>>
    where
        T::Bytes: Default;

    /// Writes data to a register.
    /// # Safety
    /// The implementation must ensure that the register access is valid.
    /// Since this function can mutate data without an immutable reference,
    /// e.g. using MMIO, this function cannot be possibly safe.
    unsafe fn write_reg<T: PrimInt + ToBytes>(&self, reg: Register<T>, value: T) -> Option<()>
    where
        T::Bytes: Default;
}

/// A hardware register mapped in the current address space.
/// All reads and writes must be properly aligned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Register<T: PrimInt> {
    offset: usize,
    native_endian: bool,
    _p: PhantomData<T>,
}

impl<T: PrimInt> Register<T> {
    /// Creates a new register with native endianness.
    /// `offset` is in units of bytes.
    pub const fn new(offset: usize) -> Self {
        assert!(
            offset.is_multiple_of(size_of::<T>()),
            "A register must be aligned to a multiple of its size"
        );
        Self {
            _p: PhantomData,
            offset,
            native_endian: true,
        }
    }

    /// Marks this field as little endian.
    pub const fn with_le(mut self) -> Self {
        self.native_endian = is_little_endian();
        self
    }

    /// Marks this field as little endian.
    pub const fn with_be(mut self) -> Self {
        self.native_endian = !is_little_endian();
        self
    }

    pub const fn offset(&self) -> usize {
        self.offset
    }
}

/// A [`Field`] is a subtype contained in a [`Register`].
/// `T` is a register type, and `A` is the type of the relevant part of that register.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field<T: PrimInt, A: PrimInt> {
    field_offset: usize,
    bit_width: usize,
    register: Register<T>,
    _p: PhantomData<T>,
    _a: PhantomData<A>,
}

impl<T: PrimInt, A: PrimInt> Field<T, A> {
    /// Creates a new field with native endianness.
    pub const fn new(register: Register<T>, field_offset: usize) -> Self {
        assert!((field_offset + size_of::<A>()) <= size_of::<T>());
        Self {
            _p: PhantomData,
            _a: PhantomData,
            register,
            field_offset: field_offset * 8,
            bit_width: size_of::<A>() * 8,
        }
    }

    /// Creates a new field spanning the given bit range (inclusive).
    pub const fn new_bits(register: Register<T>, range: RangeInclusive<usize>) -> Self {
        let start = *range.start();
        let end = *range.end();
        assert!(start <= end);
        assert!(
            end < size_of::<T>() * u8::BITS as usize,
            "T is not large enough to store the field's value"
        );
        let width = end - start + 1;
        assert!(
            width <= size_of::<A>() * u8::BITS as usize,
            "A is not large enough to store the field's value"
        );
        Self {
            _a: PhantomData,
            _p: PhantomData,
            register,
            field_offset: start,
            bit_width: width,
        }
    }

    pub const fn byte_offset(&self) -> usize {
        self.register.offset() + (self.field_offset / 8)
    }

    pub const fn bit_offset(&self) -> usize {
        self.field_offset
    }

    pub const fn bit_width(&self) -> usize {
        self.bit_width
    }
}

const fn is_little_endian() -> bool {
    #[cfg(target_endian = "little")]
    return true;
    #[cfg(target_endian = "big")]
    return false;
}

impl MemoryView for [u8] {
    fn read_reg<T: PrimInt + FromBytes>(&self, reg: Register<T>) -> Option<BitValue<T>>
    where
        T::Bytes: Default,
    {
        let buf = self.get(reg.offset()..reg.offset() + size_of::<T>())?;
        let mut bytes = T::Bytes::default();
        bytes.as_mut().copy_from_slice(buf);
        let mut num = T::from_ne_bytes(&bytes);
        if !reg.native_endian {
            num = num.swap_bytes();
        }
        Some(BitValue::new(num))
    }

    fn write_reg<T: PrimInt + ToBytes>(&mut self, reg: Register<T>, value: T) -> Option<()>
    where
        T::Bytes: Default,
    {
        let mut v = value;
        if !reg.native_endian {
            v = v.swap_bytes();
        }
        let bytes = T::to_ne_bytes(&v);
        let buf = self.get_mut(reg.offset()..reg.offset() + size_of::<T>())?;
        buf.copy_from_slice(bytes.as_ref());
        Some(())
    }
}

/// Represents a region of memory mapped IO.
#[derive(Debug)]
pub struct MmioView {
    /// A pointer to the start of this region in virtual memory.
    base: *mut (),
    /// The length of this region in bytes.
    len: usize,
}

unsafe impl Send for MmioView {}
unsafe impl Sync for MmioView {}

impl MmioView {
    /// Creates a new MMIO context over device memory.
    /// # Safety
    /// `phys` must be pointing to the start of the device memory region.
    pub unsafe fn new(phys: PhysAddr, len: usize) -> Self {
        return Self {
            // TODO: When adding memory type support, map this as uncacheable.
            base: PageTable::get_kernel()
                .map_memory::<KernelAlloc>(phys, VmFlags::Read | VmFlags::Write, len)
                .unwrap() as *mut (),
            len,
        };
    }

    pub fn sub_view(&self, offset: usize) -> Option<MmioSubView<'_>> {
        if offset >= self.len {
            return None;
        }

        Some(MmioSubView {
            parent: self,
            offset,
        })
    }

    pub fn base(&self) -> *mut () {
        self.base
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn do_read_reg<T: PrimInt>(&self, reg: Register<T>, offset: usize) -> Option<BitValue<T>> {
        if reg.offset() + offset + size_of::<T>() > self.len {
            return None;
        }

        let mut value = unsafe {
            (self.base as *mut T)
                .byte_add(reg.offset() + offset)
                .read_volatile()
        };
        if !reg.native_endian {
            value = value.swap_bytes();
        }

        Some(BitValue::new(value))
    }

    fn do_write_reg<T: PrimInt>(&self, reg: Register<T>, value: T, offset: usize) -> Option<()> {
        if reg.offset() + offset + size_of::<T>() > self.len {
            return None;
        }

        let value = match reg.native_endian {
            true => value,
            false => value.swap_bytes(),
        };
        unsafe {
            (self.base as *mut T)
                .byte_add(reg.offset() + offset)
                .write_volatile(value)
        };
        Some(())
    }
}

impl Drop for MmioView {
    fn drop(&mut self) {
        PageTable::get_kernel()
            .unmap_range::<KernelAlloc>(VirtAddr(self.base as usize), self.len)
            .unwrap();
    }
}

impl UnsafeMemoryView for MmioView {
    unsafe fn read_reg<T: PrimInt>(&self, reg: Register<T>) -> Option<BitValue<T>> {
        self.do_read_reg(reg, 0)
    }

    unsafe fn write_reg<T: PrimInt>(&self, reg: Register<T>, value: T) -> Option<()> {
        self.do_write_reg(reg, value, 0)
    }
}

pub struct MmioSubView<'a> {
    parent: &'a MmioView,
    offset: usize,
}

impl MmioSubView<'_> {
    pub fn sub_view(&self, offset: usize) -> Option<Self> {
        self.parent.sub_view(self.offset + offset)
    }

    pub fn base(&self) -> *mut () {
        unsafe { self.parent.base().byte_add(self.offset) }
    }

    pub fn len(&self) -> usize {
        self.parent.len() - self.offset
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl UnsafeMemoryView for MmioSubView<'_> {
    unsafe fn read_reg<T: PrimInt>(&self, reg: Register<T>) -> Option<BitValue<T>> {
        self.parent.do_read_reg(reg, self.offset)
    }

    unsafe fn write_reg<T: PrimInt>(&self, reg: Register<T>, value: T) -> Option<()> {
        self.parent.do_write_reg(reg, value, self.offset)
    }
}
