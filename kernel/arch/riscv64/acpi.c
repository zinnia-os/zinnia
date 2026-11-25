// SPDX-License-Identifier: BSD-3-Clause

#include <uacpi/kernel_api.h>
#include <uacpi/status.h>

uacpi_status uacpi_kernel_io_map(uacpi_io_addr base, uacpi_size len, uacpi_handle* out_handle) {
    return UACPI_STATUS_UNIMPLEMENTED;
}

void uacpi_kernel_io_unmap(uacpi_handle handle) {}

#define IO_READ(width) \
    uacpi_status uacpi_kernel_io_read##width(uacpi_handle handle, uacpi_size offset, uacpi_u##width* out_value) { \
        return UACPI_STATUS_UNIMPLEMENTED; \
    }

#define IO_WRITE(width) \
    uacpi_status uacpi_kernel_io_write##width(uacpi_handle handle, uacpi_size offset, uacpi_u##width in_value) { \
        return UACPI_STATUS_UNIMPLEMENTED; \
    }

IO_READ(8)
IO_READ(16)
IO_READ(32)
IO_WRITE(8)
IO_WRITE(16)
IO_WRITE(32)
