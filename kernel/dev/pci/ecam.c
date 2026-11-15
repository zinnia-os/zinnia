// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "pci: " fmt

#include <arch/mem.h>
#include <dev/pci/pci.h>
#include <lib/mmio.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>

struct pci_ecam {
    struct pci_access access;
    void* base;
};

static void* ecam_ptr(struct pci_access* self, const struct pci_address* addr, uint16_t offset) {
    struct pci_ecam* ecam = containerof(self, struct pci_ecam, access);
    size_t index = (size_t)(addr->bus - self->start_bus) << 20 | (size_t)(addr->device & 0x1f) << 15 |
        (size_t)(addr->function & 0x7) << 12 | (offset & 0xffc);
    return (uint8_t*)ecam->base + index;
}

static uint32_t ecam_read32(struct pci_access* self, const struct pci_address* addr, uint16_t offset) {
    return mmio_read32(ecam_ptr(self, addr, offset));
}

static void ecam_write32(struct pci_access* self, const struct pci_address* addr, uint16_t offset, uint32_t value) {
    mmio_write32(ecam_ptr(self, addr, offset), value);
}

errno_t pci_ecam_register(phys_t base, uint16_t domain, uint8_t start_bus, uint8_t end_bus) {
    if (end_bus < start_bus)
        return EINVAL;

    size_t length = ((size_t)(end_bus - start_bus) + 1) << 20;
    phys_t phys = base + ((phys_t)start_bus << 20);

    uintptr_t vaddr = vm_alloc(length);
    if (!vaddr)
        return ENOMEM;

    for (size_t i = 0; i < length; i += arch_mem_page_size()) {
        errno_t status = pmap_map(&kernel_vma.pmap, vaddr + i, phys + i, PTE_READ | PTE_WRITE, CACHE_MMIO);
        if (status != 0) {
            while (i) {
                i -= arch_mem_page_size();
                pmap_unmap(&kernel_vma.pmap, vaddr + i);
            }
            vm_free(vaddr, length);
            return status;
        }
    }

    struct pci_ecam* ecam = mem_alloc(sizeof(*ecam), 0);
    if (ecam == nullptr)
        return ENOMEM;

    *ecam = (struct pci_ecam){
        .access =
            {
                .name = "ecam",
                .domain = domain,
                .start_bus = start_bus,
                .end_bus = end_bus,
                .priority = PCI_ACCESS_PRIO_ECAM,
                .read32 = ecam_read32,
                .write32 = ecam_write32,
            },
        .base = (void*)vaddr,
    };

    errno_t status = pci_access_register(&ecam->access);
    if (status != 0) {
        mem_free(ecam, sizeof(*ecam));
        return status;
    }

    log("ECAM for %.4hx:%.2hhx - %.4hx:%.2hhx at %p\n", domain, start_bus, domain, end_bus, (void*)phys);
    return 0;
}
