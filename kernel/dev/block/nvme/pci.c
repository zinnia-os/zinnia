// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "nvme: " fmt

#include <arch/mem.h>
#include <dev/block/nvme/nvme.h>
#include <dev/pci/pci.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/irq.h>
#include <lib/module.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stdcountof.h>

static _Atomic(size_t) nvme_counter;

struct nvme_irq_handler {
    struct irq_handler handler;
    struct nvme_ctrl* controller;
};

static void nvme_irq_thread(struct irq_handler* self) {
    struct nvme_irq_handler* handler = containerof(self, struct nvme_irq_handler, handler);
    struct nvme_io_queue* queue = nvme_ctrl_io_queue(handler->controller);
    if (queue)
        nvme_io_queue_drain(queue);
}

static errno_t map_bar(struct pci_bar* bar, void** out) {
    const size_t page_size = arch_mem_page_size();
    const size_t len = ROUND_UP(bar->size, page_size);
    uintptr_t vaddr = vm_alloc(len);
    if (!vaddr)
        return ENOMEM;

    for (size_t i = 0; i < len; i += page_size) {
        errno_t status = pmap_map(&kernel_vma.pmap, vaddr + i, bar->address + i, PTE_READ | PTE_WRITE, CACHE_MMIO);
        if (status) {
            while (i) {
                i -= page_size;
                pmap_unmap(&kernel_vma.pmap, vaddr + i);
            }
            vm_free(vaddr, len);
            return status;
        }
    }

    *out = (void*)vaddr;
    return 0;
}

static errno_t nvme_pci_probe(struct pci_device* dev, const struct pci_match* match) {
    pci_log(dev, "Probing NVMe device\n");

    // Enable MMIO decoding and DMA. We only support MSI-X or polling, so keep legacy INTx disabled.
    uint16_t cmd = pci_read16(dev, PCI_COMMAND);
    pci_write16(dev, PCI_COMMAND, cmd | PCI_COMMAND_MEM | PCI_COMMAND_MASTER | PCI_COMMAND_INTX_DISABLE);

    struct irq_line* irq_line = nullptr;
    if (pci_setup_msix(dev, &irq_line)) {
        pci_log_warn(dev, "NVMe MSI-X setup failed, falling back to polling completions\n");
        irq_line = nullptr;
    }

    struct pci_bar bar;
    errno_t status = pci_bar_get(dev, 0, &bar);
    if (status)
        return ENXIO;
    if (bar.type != PCI_BAR_MMIO32 && bar.type != PCI_BAR_MMIO64)
        return ENXIO;

    void* regs;
    status = map_bar(&bar, &regs);
    if (status)
        return status;

    struct nvme_ctrl* controller;
    status = nvme_ctrl_new_pci(regs, dev->dma_tag, &controller);
    if (status) {
        pci_log_err(dev, "Failed to probe controller: %i\n", status);
        return ENODEV;
    }

    if (irq_line) {
        struct nvme_irq_handler* handler = mem_alloc(sizeof(*handler), 0);
        if (!handler) {
            mem_free(controller, sizeof(*controller));
            return ENOMEM;
        }
        handler->handler.thread = nvme_irq_thread;
        handler->controller = controller;

        char name[32];
        ksnprintf(name, sizeof(name), "nvme-" PCI_ADDR_FMT, PCI_ADDR_ARGS(&dev->address));
        status = irq_line_attach_task(irq_line, &handler->handler, name);
        if (status) {
            mem_free(handler, sizeof(*handler));
            return status;
        }
        if (irq_line->unmask)
            irq_line->unmask(irq_line);
    }

    // Reset the controller to initialize all queues and other structures.
    status = nvme_ctrl_reset(controller, irq_line != nullptr, 0);
    if (status) {
        pci_log_err(dev, "Failed to reset controller: %i\n", status);
        return ENODEV;
    }

    status = nvme_ctrl_identify(controller);
    if (status) {
        pci_log_err(dev, "Failed to identify controller: %i\n", status);
        return ENODEV;
    }

    nvme_ns_vec_t namespaces;
    status = nvme_ctrl_scan_namespaces(controller, &namespaces);
    if (status) {
        pci_log_err(dev, "Failed to identify controller: %i\n", status);
        return ENODEV;
    }

    const size_t nvme_id = atomic_fetch_add(&nvme_counter, 1, ATOMIC_SEQ_CST);
    vec_foreach(&namespaces, i) {
        struct nvme_ns* ns = vec_elem_get(&namespaces, i);
        char path[32];
        ksnprintf(path, sizeof(path), "nvme%zun%u", nvme_id, ns->nsid);
        status = blockdev_register(path, &ns->blk);
        if (status)
            break;
    }
    vec_destroy(&namespaces);

    return status;
}

static const struct pci_match nvme_match[] = {
    // Generic NVMe device. Assume no quirks or any funny business.
    {PCI_MATCH_CLASS(1, 8, 2), .data = 0},
};

MATCH_TABLE("pci", nvme_match);

static struct pci_driver nvme_driver = {
    .name = "nvme",
    .match = nvme_match,
    .num_matches = countof(nvme_match),
    .probe = nvme_pci_probe,
};

errno_t nvme_init(struct module* mod) {
    return pci_driver_register(&nvme_driver);
}

errno_t nvme_exit(struct module* mod) {
    // TODO: Unregister
    return 0;
}

MODULE_ENTRY(nvme_init);
MODULE_EXIT(nvme_exit);

MODULE_DESC("NVMe Block Devices");
MODULE_AUTHOR("Marvin Friedrich");
MODULE_LICENSE("BSD-3-Clause");
MODULE_VERSION("1.0");
