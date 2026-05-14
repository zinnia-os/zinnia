#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused)]

use crate::{
    arch, clock,
    device::pci::{Access, Address, ACCESS},
    memory::{
        free, malloc,
        pmm::KernelAlloc,
        virt::{mmu::PageTable, VmFlags},
    },
    util::{self, spin::SpinLock},
};
use alloc::{alloc::GlobalAlloc, boxed::Box};
use core::{
    ffi::{c_void, CStr},
    mem::forget,
    ptr::null_mut,
};

pub use uacpi_sys::*;

#[unsafe(no_mangle)]
unsafe extern "C" fn uacpi_kernel_get_rsdp(out_rsdp_address: *mut uacpi_phys_addr) -> uacpi_status {
    unsafe { *out_rsdp_address = super::RSDP_ADDRESS.get().value() as u64 };
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_map(addr: uacpi_phys_addr, len: uacpi_size) -> *mut c_void {
    let aligned_addr = util::align_down(addr as usize, arch::virt::get_page_size());
    let difference = (addr as usize - aligned_addr);
    let aligned_len = util::align_up(len + difference, arch::virt::get_page_size());
    return unsafe {
        PageTable::get_kernel()
            .map_memory::<KernelAlloc>(
                aligned_addr.into(),
                VmFlags::Read | VmFlags::Write,
                aligned_len,
            )
            .unwrap()
            .byte_add(difference)
    } as *mut c_void;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_unmap(addr: *mut c_void, len: uacpi_size) {
    // TODO
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_log(arg1: uacpi_log_level, arg2: *const uacpi_char) {
    let msg = unsafe { CStr::from_ptr(arg2) }.to_str().unwrap();
    // uACPI prints a newline at the end, so we need to print it without.
    match arg1 {
        UACPI_LOG_ERROR => log_inner!("\x1b[1;31m", "\x1b[0m", "{}", msg),
        UACPI_LOG_WARN => log_inner!("\x1b[1;33m", "\x1b[0m", "{}", msg),
        _ => log_inner!("", "", "{}", msg),
    }
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_device_open(
    address: uacpi_pci_address,
    out_handle: *mut uacpi_handle,
) -> uacpi_status {
    let address = Box::new(Address {
        segment: address.segment,
        bus: address.bus,
        slot: address.device,
        function: address.function,
    });
    unsafe { out_handle.write_unaligned(Box::into_raw(address) as _) };
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_device_close(arg1: uacpi_handle) {
    // This function intentionally left blank.
}

fn pci_access_from_address(address: Address) -> Option<&'static dyn Access> {
    ACCESS
        .get()
        .iter()
        .find(|x| x.decodes(address))
        .map(|x| x.as_ref())
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_read8(
    device: uacpi_handle,
    offset: uacpi_size,
    value: *mut uacpi_u8,
) -> uacpi_status {
    let ptr = device as *const Address;
    let address = unsafe { ptr.read() };
    let access = match pci_access_from_address(address) {
        Some(x) => x,
        None => return UACPI_STATUS_NOT_FOUND,
    };
    unsafe { value.write(access.read8(address, offset as u32)) };
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_read16(
    device: uacpi_handle,
    offset: uacpi_size,
    value: *mut uacpi_u16,
) -> uacpi_status {
    let ptr = device as *const Address;
    let address = unsafe { ptr.read() };
    let access = match pci_access_from_address(address) {
        Some(x) => x,
        None => return UACPI_STATUS_NOT_FOUND,
    };
    unsafe { value.write(access.read16(address, offset as u32)) };
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_read32(
    device: uacpi_handle,
    offset: uacpi_size,
    value: *mut uacpi_u32,
) -> uacpi_status {
    let ptr = device as *const Address;
    let address = unsafe { ptr.read() };
    let access = match pci_access_from_address(address) {
        Some(x) => x,
        None => return UACPI_STATUS_NOT_FOUND,
    };
    unsafe { value.write(access.read32(address, offset as u32)) };
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_write8(
    device: uacpi_handle,
    offset: uacpi_size,
    value: uacpi_u8,
) -> uacpi_status {
    let ptr = device as *const Address;
    let address = unsafe { ptr.read() };
    let access = match pci_access_from_address(address) {
        Some(x) => x,
        None => return UACPI_STATUS_NOT_FOUND,
    };
    access.write8(address, offset as u32, value);
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_write16(
    device: uacpi_handle,
    offset: uacpi_size,
    value: uacpi_u16,
) -> uacpi_status {
    let ptr = device as *const Address;
    let address = unsafe { ptr.read() };
    let access = match pci_access_from_address(address) {
        Some(x) => x,
        None => return UACPI_STATUS_NOT_FOUND,
    };
    access.write16(address, offset as u32, value);
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_pci_write32(
    device: uacpi_handle,
    offset: uacpi_size,
    value: uacpi_u32,
) -> uacpi_status {
    let ptr = device as *const Address;
    let address = unsafe { ptr.read() };
    let access = match pci_access_from_address(address) {
        Some(x) => x,
        None => return UACPI_STATUS_NOT_FOUND,
    };
    access.write32(address, offset as u32, value);
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_alloc(size: uacpi_size) -> *mut c_void {
    if size == 0 {
        return null_mut();
    }

    return unsafe { malloc(size) };
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_free(mem: *mut c_void, size: uacpi_size) {
    // Frees of NULL are a no-op in uACPI.
    if mem.is_null() {
        return;
    }

    unsafe { free(mem, size) };
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_get_nanoseconds_since_boot() -> uacpi_u64 {
    return clock::get_elapsed() as uacpi_u64;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_stall(usec: uacpi_u8) {
    clock::block_ns(usec as usize);
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_sleep(msec: uacpi_u64) {
    todo!()
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_create_mutex() -> uacpi_handle {
    // TODO
    let mut b = Box::new(0);
    return Box::into_raw(b) as uacpi_handle;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_free_mutex(arg1: uacpi_handle) {
    // TODO
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_create_event() -> uacpi_handle {
    // TODO
    let mut b = Box::new(0);
    return Box::into_raw(b) as uacpi_handle;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_free_event(arg1: uacpi_handle) {
    todo!()
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_get_thread_id() -> uacpi_thread_id {
    // TODO
    return 0 as uacpi_thread_id;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_acquire_mutex(arg1: uacpi_handle, arg2: uacpi_u16) -> uacpi_status {
    // TODO
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_release_mutex(arg1: uacpi_handle) {
    // TODO
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_wait_for_event(arg1: uacpi_handle, arg2: uacpi_u16) -> uacpi_bool {
    todo!()
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_signal_event(arg1: uacpi_handle) {
    todo!()
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_reset_event(arg1: uacpi_handle) {
    todo!()
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_handle_firmware_request(
    arg1: *mut uacpi_firmware_request,
) -> uacpi_status {
    todo!()
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_install_interrupt_handler(
    irq: uacpi_u32,
    arg1: uacpi_interrupt_handler,
    ctx: uacpi_handle,
    out_irq_handle: *mut uacpi_handle,
) -> uacpi_status {
    // TODO
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_uninstall_interrupt_handler(
    arg1: uacpi_interrupt_handler,
    irq_handle: uacpi_handle,
) -> uacpi_status {
    // TODO
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_create_spinlock() -> uacpi_handle {
    let b = Box::new(SpinLock::new());
    return Box::into_raw(b) as uacpi_handle;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_free_spinlock(arg1: uacpi_handle) {
    let b = unsafe { Box::from_raw(arg1 as *mut SpinLock) };
    drop(b);
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_lock_spinlock(arg1: uacpi_handle) -> uacpi_cpu_flags {
    let spin = unsafe { (arg1 as *const SpinLock).as_ref().unwrap() };
    spin.lock();
    return 0;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_unlock_spinlock(arg1: uacpi_handle, arg2: uacpi_cpu_flags) {
    let spin = unsafe { (arg1 as *const SpinLock).as_ref().unwrap() };
    spin.unlock();
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_schedule_work(
    arg1: uacpi_work_type,
    arg2: uacpi_work_handler,
    ctx: uacpi_handle,
) -> uacpi_status {
    // TODO
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_wait_for_work_completion() -> uacpi_status {
    // TODO
    return UACPI_STATUS_OK;
}
