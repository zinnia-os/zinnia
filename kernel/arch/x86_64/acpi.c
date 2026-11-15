// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/asm.h>
#include <stdckdint.h>
#include <stdint.h>
#include <uacpi/kernel_api.h>
#include <uacpi/status.h>

uacpi_status uacpi_kernel_io_map(uacpi_io_addr base, uacpi_size len, uacpi_handle* out_handle) {
    uintptr_t end;
    if (ckd_add(&end, base, len) || end > 0x10000)
        return UACPI_STATUS_INVALID_ARGUMENT;
    *out_handle = (uacpi_handle)(uintptr_t)base;
    return UACPI_STATUS_OK;
}

void uacpi_kernel_io_unmap(uacpi_handle handle) {
    // Left intentionally blank.
}

#define IO_READ(width, suffix) \
    uacpi_status uacpi_kernel_io_read##width(uacpi_handle handle, uacpi_size offset, uacpi_u##width* out_value) { \
        *out_value = asm_in##suffix((uintptr_t)handle + offset); \
        return UACPI_STATUS_OK; \
    }

#define IO_WRITE(width, suffix) \
    uacpi_status uacpi_kernel_io_write##width(uacpi_handle handle, uacpi_size offset, uacpi_u##width in_value) { \
        asm_out##suffix((uintptr_t)handle + offset, in_value); \
        return UACPI_STATUS_OK; \
    }

IO_READ(8, b)
IO_READ(16, w)
IO_READ(32, l)
IO_WRITE(8, b)
IO_WRITE(16, w)
IO_WRITE(32, l)
