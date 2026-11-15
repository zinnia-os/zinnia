// SPDX-License-Identifier: BSD-3-Clause

#include <arch/irq.h>
#include <arch/mem.h>
#include <dev/acpi/acpi.h>
#include <dev/pci/pci.h>
#include <lib/clock.h>
#include <lib/console.h>
#include <lib/init.h>
#include <lib/irq.h>
#include <lib/mmio.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/semaphore.h>
#include <lib/spin.h>
#include <lib/utils.h>
#include <lib/workqueue.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stdarg.h>
#include <uacpi/kernel_api.h>
#include <uacpi/status.h>
#include <uacpi/types.h>

uacpi_status uacpi_kernel_get_rsdp(uacpi_phys_addr* out_rsdp_address) {
    *out_rsdp_address = (uacpi_phys_addr)acpi_rsdp_addr;
    return UACPI_STATUS_OK;
}

void* uacpi_kernel_map(uacpi_phys_addr addr, uacpi_size len) {
    uintptr_t aligned_addr = ROUND_DOWN(addr, arch_mem_page_size());
    uintptr_t difference = addr - aligned_addr;
    size_t aligned_len = ROUND_UP(len + difference, arch_mem_page_size());

    uintptr_t vaddr = vm_alloc(aligned_len);
    if (!vaddr)
        return nullptr;

    for (size_t i = 0; i < aligned_len; i += arch_mem_page_size()) {
        pmap_map(&kernel_vma.pmap, vaddr + i, aligned_addr + i, PTE_READ | PTE_WRITE, CACHE_ANY);
    }

    return (void*)(vaddr + difference);
}

void uacpi_kernel_unmap(void* addr, uacpi_size len) {
    uintptr_t aligned_addr = ROUND_DOWN((uintptr_t)addr, arch_mem_page_size());
    uintptr_t difference = (uintptr_t)addr - aligned_addr;
    size_t aligned_len = ROUND_UP(len + difference, arch_mem_page_size());

    for (size_t i = 0; i < aligned_len; i += arch_mem_page_size())
        pmap_unmap(&kernel_vma.pmap, aligned_addr + i);
    vm_free(aligned_addr, aligned_len);
}

static enum log_level acpi_log_level(uacpi_log_level lvl) {
    switch (lvl) {
    case UACPI_LOG_ERROR:
        return LOG_ERR;

    case UACPI_LOG_WARN:
        return LOG_WARN;

    case UACPI_LOG_INFO:
        return LOG_INFO;

    default:
        return LOG_DEBUG;
    }
}

void uacpi_kernel_log(uacpi_log_level lvl, const uacpi_char* msg, ...) {
    va_list args;
    va_start(args, msg);
    log_vwrite_prefixed(acpi_log_level(lvl), "acpi: ", msg, args);
    va_end(args);
}

void uacpi_kernel_vlog(uacpi_log_level lvl, const uacpi_char* msg, uacpi_va_list args) {
    log_vwrite_prefixed(acpi_log_level(lvl), "acpi: ", msg, args);
}

uacpi_status uacpi_kernel_pci_device_open(uacpi_pci_address address, uacpi_handle* out_handle) {
#if CONFIG_PCI
    struct pci_address addr = {
        .domain = address.segment,
        .bus = address.bus,
        .device = address.device,
        .function = address.function,
    };
    struct pci_access* access = pci_access_find(&addr);
    if (!access)
        return UACPI_STATUS_NOT_FOUND;

    struct pci_device* dev = mem_alloc(sizeof(*dev), 0);
    if (!dev)
        return UACPI_STATUS_OUT_OF_MEMORY;
    dev->address = addr;
    dev->access = access;
    *out_handle = dev;
    return UACPI_STATUS_OK;
#else
    return UACPI_STATUS_NOT_FOUND;
#endif
}

void uacpi_kernel_pci_device_close(uacpi_handle handle) {
    mem_free(handle, sizeof(struct pci_device));
}

#if CONFIG_PCI
#define PCI_READ(width) \
    uacpi_status uacpi_kernel_pci_read##width(uacpi_handle device, uacpi_size offset, uacpi_u##width* value) { \
        *value = pci_read##width(device, offset); \
        return UACPI_STATUS_OK; \
    }
#define PCI_WRITE(width) \
    uacpi_status uacpi_kernel_pci_write##width(uacpi_handle device, uacpi_size offset, uacpi_u##width value) { \
        pci_write##width(device, offset, value); \
        return UACPI_STATUS_OK; \
    }
#else
#define PCI_READ(width) \
    uacpi_status uacpi_kernel_pci_read##width(uacpi_handle device, uacpi_size offset, uacpi_u##width* value) { \
        return UACPI_STATUS_UNIMPLEMENTED; \
    }
#define PCI_WRITE(width) \
    uacpi_status uacpi_kernel_pci_write##width(uacpi_handle device, uacpi_size offset, uacpi_u##width value) { \
        return UACPI_STATUS_UNIMPLEMENTED; \
    }
#endif

PCI_READ(8)
PCI_READ(16)
PCI_READ(32)
PCI_WRITE(8)
PCI_WRITE(16)
PCI_WRITE(32)

void* uacpi_kernel_alloc(uacpi_size size) {
    return mem_alloc(size, ALLOC_NOZERO);
}

void uacpi_kernel_free(void* mem, uacpi_size size) {
    mem_free(mem, size);
}

uacpi_u64 uacpi_kernel_get_nanoseconds_since_boot() {
    return clock_get_elapsed_ns();
}

void uacpi_kernel_stall(uacpi_u8 usec) {
    clock_spin_ns(usec * 1000ull);
}

void uacpi_kernel_sleep(uacpi_u64 msec) {
    sched_sleep_until(clock_get_elapsed_ns() + msec * 1'000'000);
}

uacpi_handle uacpi_kernel_create_semaphore(uacpi_u32 initial_units) {
    sem_t* sem = mem_alloc(sizeof(*sem), 0);
    if (sem)
        sem_init(sem, initial_units);
    return sem;
}

void uacpi_kernel_free_semaphore(uacpi_handle handle) {
    mem_free(handle, sizeof(sem_t));
}

uacpi_status uacpi_kernel_wait_for_semaphore(uacpi_handle handle, uacpi_u16 timeout) {
    sem_t* sem = handle;
    if (timeout == 0)
        return sem_trywait(sem) ? UACPI_STATUS_OK : UACPI_STATUS_TIMEOUT;
    if (timeout == 0xFFFF) {
        sem_wait(sem);
        return UACPI_STATUS_OK;
    }

    const uint64_t deadline = clock_get_elapsed_ns() + timeout * 1'000'000ull;
    return sem_timedwait(sem, &deadline) ? UACPI_STATUS_OK : UACPI_STATUS_TIMEOUT;
}

void uacpi_kernel_signal_semaphore(uacpi_handle handle) {
    sem_post(handle);
}

uacpi_thread_id uacpi_kernel_get_thread_id() {
    return percpu_get()->sched.current;
}

uacpi_interrupt_state uacpi_kernel_disable_interrupts() {
    irq_lock();
    return 0;
}

void uacpi_kernel_restore_interrupts(uacpi_interrupt_state) {
    irq_unlock();
}

uacpi_status uacpi_kernel_handle_firmware_request(uacpi_firmware_request* req) {
    switch (req->type) {
    case UACPI_FIRMWARE_REQUEST_TYPE_BREAKPOINT:
        log_warn("acpi: Ignoring AML breakpoint\n");
        break;
    case UACPI_FIRMWARE_REQUEST_TYPE_FATAL:
        log_err(
            "acpi: Fatal firmware error: type %u, code %#x, arg %#llx\n",
            req->fatal.type,
            req->fatal.code,
            (unsigned long long)req->fatal.arg
        );
        break;
    }
    return UACPI_STATUS_OK;
}

struct acpi_irq {
    struct irq_handler handler;
    uacpi_interrupt_handler func;
    uacpi_handle ctx;
};

static enum irq_status acpi_irq_raise(struct irq_handler* self) {
    struct acpi_irq* irq = containerof(self, struct acpi_irq, handler);
    return irq->func(irq->ctx) == UACPI_INTERRUPT_HANDLED ? IRQ_STATUS_HANDLED : IRQ_STATUS_IGNORED;
}

uacpi_status uacpi_kernel_install_interrupt_handler(
    uacpi_u32 irq,
    uacpi_interrupt_handler handler,
    uacpi_handle ctx,
    uacpi_handle* out_irq_handle
) {
    uint32_t gsi = irq;
    enum irq_trigger_mode trigger = IRQ_TRIGGER_LEVEL;
    enum irq_polarity polarity = IRQ_POLARITY_LOW;
    if (irq <= UINT8_MAX)
        arch_irq_isa_override(irq, &gsi, &trigger, &polarity);

    struct irq_line* line;
    errno_t status = arch_irq_get_gsi(gsi, &line);
    if (status) {
        log_err("acpi: Unable to get GSI %u for IRQ %u (%i)\n", gsi, irq, status);
        return UACPI_STATUS_UNIMPLEMENTED;
    }

    struct acpi_irq* acpi_irq = mem_alloc(sizeof(*acpi_irq), 0);
    if (!acpi_irq)
        return UACPI_STATUS_OUT_OF_MEMORY;
    acpi_irq->handler.raise = acpi_irq_raise;
    acpi_irq->func = handler;
    acpi_irq->ctx = ctx;

    irq_line_attach(line, &acpi_irq->handler);
    irq_line_program(line, trigger, polarity);
    log("acpi: Installed handler for IRQ %u on GSI %u\n", irq, gsi);

    *out_irq_handle = acpi_irq;
    return UACPI_STATUS_OK;
}

uacpi_status uacpi_kernel_uninstall_interrupt_handler(uacpi_interrupt_handler handler, uacpi_handle irq_handle) {
    struct acpi_irq* acpi_irq = irq_handle;
    struct irq_line* line = acpi_irq->handler.line;
    irq_line_detach(line, &acpi_irq->handler);
    if (list_is_empty(&line->handlers) && line->mask)
        line->mask(line);
    mem_free(acpi_irq, sizeof(*acpi_irq));
    return UACPI_STATUS_OK;
}

uacpi_handle uacpi_kernel_create_spinlock() {
    struct spinlock* lock = mem_alloc(sizeof(*lock), 0);
    if (lock)
        spin_init(lock);
    return lock;
}

void uacpi_kernel_free_spinlock(uacpi_handle handle) {
    mem_free(handle, sizeof(struct spinlock));
}

uacpi_cpu_flags uacpi_kernel_lock_spinlock(uacpi_handle handle) {
    spin_lock(handle);
    return 0;
}

void uacpi_kernel_unlock_spinlock(uacpi_handle handle, uacpi_cpu_flags) {
    spin_unlock(handle);
}

struct acpi_work {
    struct work work;
    uacpi_work_handler handler;
    uacpi_handle ctx;
};

static void acpi_work_fn(struct work* work) {
    struct acpi_work* item = containerof(work, struct acpi_work, work);
    const uacpi_work_handler handler = item->handler;
    const uacpi_handle ctx = item->ctx;
    handler(ctx);
}

uacpi_handle uacpi_kernel_create_work_item() {
    struct acpi_work* item = mem_alloc(sizeof(*item), 0);
    if (item)
        work_init(&item->work, acpi_work_fn);
    return item;
}

void uacpi_kernel_free_work_item(uacpi_handle handle) {
    mem_free(handle, sizeof(struct acpi_work));
}

void uacpi_kernel_schedule_work(uacpi_work_type, uacpi_handle work_item, uacpi_work_handler handler, uacpi_handle ctx) {
    struct acpi_work* item = work_item;
    item->handler = handler;
    item->ctx = ctx;
    work_queue(acpi_workqueue, &item->work);
}

uacpi_status uacpi_kernel_wait_for_work_completion() {
    workqueue_flush(acpi_workqueue);
    return UACPI_STATUS_OK;
}
