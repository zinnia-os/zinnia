#include <kernel/alloc.h>
#include <kernel/clock.h>
#include <kernel/init.h>
#include <kernel/mmio.h>
#include <kernel/print.h>
#include <kernel/spin.h>
#include <kernel/utils.h>
#include <stdarg.h>
#include <uacpi/kernel_api.h>
#include <uacpi/status.h>
#include <uacpi/types.h>

phys_t rsdp_addr = 0;

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

    kvprintf("uacpi: ", nullptr);
    kvprintf(msg, args);

    va_end(args);
}

#define uacpi_kernel_log(...)

void uacpi_kernel_vlog(uacpi_log_level lvl, const uacpi_char* msg, uacpi_va_list args) {
    kvprintf(msg, args);
}

uacpi_status uacpi_kernel_pci_device_open(uacpi_pci_address address, uacpi_handle* out_handle) {
    return UACPI_STATUS_UNIMPLEMENTED;
}

void uacpi_kernel_pci_device_close(uacpi_handle) {
    // TODO
}

uacpi_status uacpi_kernel_pci_read8(uacpi_handle device, uacpi_size offset, uacpi_u8* value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_pci_read16(uacpi_handle device, uacpi_size offset, uacpi_u16* value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_pci_read32(uacpi_handle device, uacpi_size offset, uacpi_u32* value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}

uacpi_status uacpi_kernel_pci_write8(uacpi_handle device, uacpi_size offset, uacpi_u8 value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_pci_write16(uacpi_handle device, uacpi_size offset, uacpi_u16 value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_pci_write32(uacpi_handle device, uacpi_size offset, uacpi_u32 value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}

uacpi_status uacpi_kernel_io_map(uacpi_io_addr base, uacpi_size len, uacpi_handle* out_handle) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
void uacpi_kernel_io_unmap(uacpi_handle handle) {
    // TODO
}

uacpi_status uacpi_kernel_io_read8(uacpi_handle, uacpi_size offset, uacpi_u8* out_value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_io_read16(uacpi_handle, uacpi_size offset, uacpi_u16* out_value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_io_read32(uacpi_handle, uacpi_size offset, uacpi_u32* out_value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_io_write8(uacpi_handle, uacpi_size offset, uacpi_u8 in_value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_io_write16(uacpi_handle, uacpi_size offset, uacpi_u16 in_value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}
uacpi_status uacpi_kernel_io_write32(uacpi_handle, uacpi_size offset, uacpi_u32 in_value) {
    return UACPI_STATUS_UNIMPLEMENTED;
}

void* uacpi_kernel_alloc(uacpi_size size) {
    return mem_alloc(size, 0);
}

void uacpi_kernel_free(void* mem) {
    mem_free(mem);
}

uacpi_u64 uacpi_kernel_get_nanoseconds_since_boot(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_get_nanoseconds_since_boot()\n");
    // TODO
    return clock_get_elapsed_ns();
}

void uacpi_kernel_stall(uacpi_u8 usec) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_stall(%hhu)\n", usec);
    // TODO
}

void uacpi_kernel_sleep(uacpi_u64 msec) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_sleep(%lu)\n", msec);
    // TODO
}

uacpi_handle uacpi_kernel_create_mutex(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_create_mutex()\n");
    // TODO
    return (uacpi_handle)mem_alloc(sizeof(int), 0);
}

void uacpi_kernel_free_mutex(uacpi_handle handle) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_free_mutex(%p)\n", handle);
    // TODO
    mem_free(handle);
}

uacpi_handle uacpi_kernel_create_event(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_create_event()\n");
    // TODO
    return mem_alloc(sizeof(int), 0);
}

void uacpi_kernel_free_event(uacpi_handle handle) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_free_event(%p)\n", handle);
    mem_free(handle);
}

uacpi_thread_id uacpi_kernel_get_thread_id(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_get_thread_id()\n");
    // TODO
    return 0;
}

uacpi_interrupt_state uacpi_kernel_disable_interrupts(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_disable_interrupts()\n");
    // TODO
    return false;
}

void uacpi_kernel_restore_interrupts(uacpi_interrupt_state state) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_disable_interrupts()\n");
    // TODO
}

uacpi_status uacpi_kernel_acquire_mutex(uacpi_handle handle, uacpi_u16 flags) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_acquire_mutex(%p, %#hx)\n", handle, flags);
    return UACPI_STATUS_OK;
}

void uacpi_kernel_release_mutex(uacpi_handle handle) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_release_mutex(%p)\n", handle);
    // TODO
}

uacpi_bool uacpi_kernel_wait_for_event(uacpi_handle handle, uacpi_u16 timeout) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_wait_for_event(%p, %hx)\n", handle, timeout);
    // TODO
    return true;
}

void uacpi_kernel_signal_event(uacpi_handle) {
    // TODO
}

void uacpi_kernel_reset_event(uacpi_handle) {
    // TODO
}

uacpi_status uacpi_kernel_handle_firmware_request(uacpi_firmware_request*) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_handle_firmware_request()\n");
    return UACPI_STATUS_UNIMPLEMENTED;
}

uacpi_status uacpi_kernel_install_interrupt_handler(
    uacpi_u32 irq,
    uacpi_interrupt_handler,
    uacpi_handle ctx,
    uacpi_handle* out_irq_handle
) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_install_interrupt_handler()\n");
    return UACPI_STATUS_UNIMPLEMENTED;
}

uacpi_status uacpi_kernel_uninstall_interrupt_handler(uacpi_interrupt_handler, uacpi_handle irq_handle) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_uninstall_interrupt_handler()\n");
    return UACPI_STATUS_UNIMPLEMENTED;
}

uacpi_handle uacpi_kernel_create_spinlock(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_create_spinlock()\n");
    // TODO
    return mem_alloc(sizeof(int), 0);
}

void uacpi_kernel_free_spinlock(uacpi_handle handle) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_free_spinlock(%p)\n", handle);
    // TODO
    mem_free(handle);
}

uacpi_cpu_flags uacpi_kernel_lock_spinlock(uacpi_handle handle) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_lock_spinlock(%p)\n", handle);
    spin_lock((struct spinlock*)handle);
    return 0;
}

void uacpi_kernel_unlock_spinlock(uacpi_handle handle, uacpi_cpu_flags flags) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_unlock_spinlock(%p, %#lx)\n", handle, flags);
    spin_unlock((struct spinlock*)handle);
}

uacpi_status uacpi_kernel_schedule_work(uacpi_work_type, uacpi_work_handler, uacpi_handle ctx) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_schedule_work()\n");
    return UACPI_STATUS_UNIMPLEMENTED;
}

uacpi_status uacpi_kernel_wait_for_work_completion(void) {
    uacpi_kernel_log(UACPI_LOG_WARN, "uacpi_kernel_wait_for_work_completion()\n");
    return UACPI_STATUS_UNIMPLEMENTED;
}
