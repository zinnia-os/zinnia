// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/asm.h>
#include <dev/pci/pci.h>
#include <lib/init.h>
#include <lib/spin.h>

constexpr uint16_t PCI_CONFIG_ADDRESS = 0xcf8;
constexpr uint16_t PCI_CONFIG_DATA = 0xcfc;

static struct spinlock port_lock = {};

static void port_select(const struct pci_address* addr, uint16_t offset) {
    asm_outl(
        PCI_CONFIG_ADDRESS,
        1u << 31 | (uint32_t)addr->bus << 16 | (uint32_t)(addr->device & 0x1f) << 11 |
            (uint32_t)(addr->function & 0x7) << 8 | (offset & 0xfc)
    );
}

static uint32_t port_read32(struct pci_access*, const struct pci_address* addr, uint16_t offset) {
    spin_lock(&port_lock);
    port_select(addr, offset);
    uint32_t value = asm_inl(PCI_CONFIG_DATA);
    spin_unlock(&port_lock);
    return value;
}

static void port_write32(struct pci_access*, const struct pci_address* addr, uint16_t offset, uint32_t value) {
    spin_lock(&port_lock);
    port_select(addr, offset);
    asm_outl(PCI_CONFIG_DATA, value);
    spin_unlock(&port_lock);
}

static struct pci_access port_access = {
    .name = "port",
    .domain = 0,
    .start_bus = 0,
    .end_bus = 255,
    .priority = PCI_ACCESS_PRIO_LEGACY,
    .read32 = port_read32,
    .write32 = port_write32,
};

[[__init]]
static void pci_x86_stage_init(struct boot_info*) {
    pci_access_register(&port_access);
}
INIT_TASK(pci_x86_stage, "arch.x86_64.pci", pci_x86_stage_init);
INIT_DEPENDS(pci_x86_stage, mem_stage);
INIT_ENTAILS(pci_x86_stage, pci_stage);
