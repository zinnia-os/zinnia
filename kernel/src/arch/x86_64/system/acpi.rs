use super::super::asm;
use crate::system::pci::{Access, Address};
use alloc::boxed::Box;
use uacpi_sys::{
    UACPI_STATUS_OK, uacpi_handle, uacpi_io_addr, uacpi_size, uacpi_status, uacpi_u8, uacpi_u16,
    uacpi_u32,
};

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_read8(
    arg1: uacpi_handle,
    offset: uacpi_size,
    out_value: *mut uacpi_u8,
) -> uacpi_status {
    unsafe {
        (*out_value) = asm::read8((arg1 as usize + offset) as u16);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_read16(
    arg1: uacpi_handle,
    offset: uacpi_size,
    out_value: *mut uacpi_u16,
) -> uacpi_status {
    unsafe {
        (*out_value) = asm::read16((arg1 as usize + offset) as u16);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_read32(
    arg1: uacpi_handle,
    offset: uacpi_size,
    out_value: *mut uacpi_u32,
) -> uacpi_status {
    unsafe {
        (*out_value) = asm::read32((arg1 as usize + offset) as u16);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_write8(
    arg1: uacpi_handle,
    offset: uacpi_size,
    in_value: uacpi_u8,
) -> uacpi_status {
    unsafe {
        asm::write8((arg1 as usize + offset) as u16, in_value);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_write16(
    arg1: uacpi_handle,
    offset: uacpi_size,
    in_value: uacpi_u16,
) -> uacpi_status {
    unsafe {
        asm::write16((arg1 as usize + offset) as u16, in_value);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_write32(
    arg1: uacpi_handle,
    offset: uacpi_size,
    in_value: uacpi_u32,
) -> uacpi_status {
    unsafe {
        asm::write32((arg1 as usize + offset) as u16, in_value);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_map(
    base: uacpi_io_addr,
    _len: uacpi_size,
    out_handle: *mut uacpi_handle,
) -> uacpi_status {
    unsafe {
        out_handle.write(base as uacpi_handle);
    }
    return UACPI_STATUS_OK;
}

#[unsafe(no_mangle)]
extern "C" fn uacpi_kernel_io_unmap(handle: uacpi_handle) {
    _ = handle;
}

struct PortIoAccess;

impl PortIoAccess {
    fn select(&self, addr: Address, offset: u32) {
        unsafe {
            asm::write32(
                0xCF8,
                (addr.bus as u32) << 16
                    | (addr.slot as u32) << 11
                    | (addr.function as u32) << 8
                    | (offset & 0xFC)
                    | 1 << 31,
            );
        }
    }
}

impl Access for PortIoAccess {
    fn segment(&self) -> u16 {
        0
    }

    fn start_bus(&self) -> u8 {
        0
    }

    fn end_bus(&self) -> u8 {
        255
    }

    fn read32(&self, addr: Address, offset: u32) -> u32 {
        self.select(addr, offset);
        unsafe { asm::read32(0xCFC) }
    }

    fn write32(&self, addr: Address, offset: u32, value: u32) {
        self.select(addr, offset);
        unsafe { asm::write32(0xCFC, value) }
    }
}

#[initgraph::task(
    name = "arch.x86_64.acpi",
    entails = [crate::system::acpi::INIT_STAGE],
    depends = [crate::memory::MEMORY_STAGE],
)]
fn ACPI_STAGE() {
    unsafe { crate::system::pci::ACCESS.init(vec![Box::new(PortIoAccess)]) };
}
