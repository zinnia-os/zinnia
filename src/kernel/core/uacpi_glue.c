#include <common/utils.h>
#include <kernel/init.h>
#include <kernel/mmio.h>
#include <kernel/print.h>
#include <stdarg.h>
#include <uacpi/kernel_api.h>
#include <uacpi/status.h>
#include <uacpi/types.h>

uacpi_status uacpi_kernel_get_rsdp(uacpi_phys_addr* out_rsdp_address) {
    *out_rsdp_address = (uacpi_phys_addr)rsdp_addr;
    return UACPI_STATUS_OK;
}

void* uacpi_kernel_map(uacpi_phys_addr addr, uacpi_size len) {
    return mmio_new(addr, len);
}

void uacpi_kernel_unmap(void* addr, uacpi_size len) {
    mmio_free(addr, len);
}

void uacpi_kernel_log(uacpi_log_level lvl, const uacpi_char* msg, ...) {
    va_list args;
    va_start(args, msg);

    kvprintf(msg, args);

    va_end(args);
}

void uacpi_kernel_vlog(uacpi_log_level lvl, const uacpi_char* msg, uacpi_va_list args) {
    kvprintf(msg, args);
}
