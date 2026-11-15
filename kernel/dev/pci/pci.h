// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/list.h>
#include <lib/print.h>
#include <lib/types.h>
#include <mem/alloc.h>
#include <mem/dma.h>
#include <stddef.h>
#include <stdint.h>

struct pci_address {
    uint16_t domain;
    uint8_t bus;
    uint8_t device;
    uint8_t function;
};

#define PCI_ADDR_FMT     "%.4hx:%.2hhx:%.2hhx.%hhx"
#define PCI_ADDR_ARGS(a) (a)->domain, (a)->bus, (a)->device, (a)->function

struct pci_id {
    uint16_t vendor_id;
    uint16_t device_id;
    uint16_t subvendor_id;
    uint16_t subdevice_id;
};

struct pci_class {
    uint8_t class;
    uint8_t subclass;
    uint8_t prog_if;
};

enum pci_cfg : uint16_t {
    PCI_VENDOR_ID = 0x00,
    PCI_DEVICE_ID = 0x02,
    PCI_COMMAND = 0x04,
    PCI_STATUS = 0x06,
    PCI_PROG_IF = 0x09,
    PCI_SUBCLASS = 0x0a,
    PCI_CLASS = 0x0b,
    PCI_HEADER_TYPE = 0x0e,
    PCI_BAR0 = 0x10,
    PCI_SECONDARY_BUS = 0x19,
    PCI_SUBSYS_VENDOR_ID = 0x2c,
    PCI_SUBSYS_ID = 0x2e,
    PCI_CAP_PTR = 0x34,
    PCI_INTERRUPT_LINE = 0x3c,
    PCI_INTERRUPT_PIN = 0x3d,
};

enum pci_command : uint16_t {
    PCI_COMMAND_IO = 1 << 0,
    PCI_COMMAND_MEM = 1 << 1,
    PCI_COMMAND_MASTER = 1 << 2,
    PCI_COMMAND_INTX_DISABLE = 1 << 10,
};

constexpr uint16_t PCI_STATUS_CAP_LIST = 1 << 4;

constexpr uint8_t PCI_HEADER_TYPE_MASK = 0x7f;
constexpr uint8_t PCI_HEADER_TYPE_MULTIFUNCTION = 0x80;
constexpr uint8_t PCI_HEADER_TYPE_BRIDGE = 0x01;

constexpr size_t PCI_NUM_BARS = 6;
constexpr uint8_t PCI_NUM_SLOTS = 32;
constexpr uint8_t PCI_NUM_FUNCTIONS = 8;

enum pci_cap_id : uint8_t {
    PCI_CAP_MSI = 0x05,
    PCI_CAP_PCIE = 0x10,
    PCI_CAP_MSIX = 0x11,
};

struct pci_access {
    const char* name;
    uint16_t domain;
    uint8_t start_bus;
    uint8_t end_bus;
    uint8_t priority;
    uint32_t (*read32)(struct pci_access* self, const struct pci_address* addr, uint16_t offset);
    void (*write32)(struct pci_access* self, const struct pci_address* addr, uint16_t offset, uint32_t value);
    list_link(struct pci_access) link;
};

enum pci_access_priority : uint8_t {
    PCI_ACCESS_PRIO_LEGACY = 0,
    PCI_ACCESS_PRIO_ECAM = 128,
};

static bool pci_access_decodes(const struct pci_access* access, const struct pci_address* addr);

errno_t pci_access_register(struct pci_access* access);
struct pci_access* pci_access_find(const struct pci_address* addr);

errno_t pci_ecam_register(phys_t base, uint16_t domain, uint8_t start_bus, uint8_t end_bus);

struct pci_driver;

struct pci_device {
    struct pci_address address;
    struct pci_id id;
    struct pci_class class;
    struct pci_access* access;
    uint8_t header_type;
    struct pci_driver* driver;
    struct dma_tag* dma_tag;
};

uint8_t pci_read8(struct pci_device* dev, uint16_t offset);
uint16_t pci_read16(struct pci_device* dev, uint16_t offset);
uint32_t pci_read32(struct pci_device* dev, uint16_t offset);
void pci_write8(struct pci_device* dev, uint16_t offset, uint8_t value);
void pci_write16(struct pci_device* dev, uint16_t offset, uint16_t value);
void pci_write32(struct pci_device* dev, uint16_t offset, uint32_t value);

enum pci_bar_type : uint8_t {
    PCI_BAR_NONE,
    PCI_BAR_MMIO32,
    PCI_BAR_MMIO64,
    PCI_BAR_IO,
};

struct pci_bar {
    enum pci_bar_type type;
    bool prefetchable;
    phys_t address;
    size_t size;
};

errno_t pci_bar_get(struct pci_device* dev, size_t index, struct pci_bar* out);

uint8_t pci_cap_next(struct pci_device* dev, uint8_t cur);
uint8_t pci_cap_find(struct pci_device* dev, uint8_t id);

constexpr uint16_t PCI_ANY_ID = 0xffff;
constexpr uint8_t PCI_ANY_CLASS = 0xff;

struct irq_line;

errno_t pci_setup_msix(struct pci_device* dev, struct irq_line** out);
errno_t pci_setup_msi(struct pci_device* dev, struct irq_line** out);
errno_t pci_setup_irq(struct pci_device* dev, struct irq_line** out);

// Matches a device by ID and class.
struct pci_match {
    uint16_t vendor_id;
    uint16_t device_id;
    uint8_t class;
    uint8_t subclass;
    uint8_t prog_if;

    // Not used to match, but can be used to pass device-specific context data.
    uintptr_t data;
};

#define PCI_MATCH_DEVICE(vendor, device) \
    .vendor_id = (vendor), .device_id = (device), .class = PCI_ANY_CLASS, .subclass = PCI_ANY_CLASS, \
    .prog_if = PCI_ANY_CLASS

#define PCI_MATCH_CLASS(c, sc, pi) \
    .vendor_id = PCI_ANY_ID, .device_id = PCI_ANY_ID, .class = (c), .subclass = (sc), .prog_if = (pi)

// Writes the address to `buf`.
static void pci_device_name(const struct pci_device* dev, char buf[static 18]);

struct pci_driver {
    const char* name;

    // Match table, array of `num_matches` entries, checked sequentially.
    // When writing a match table, the most specific matches should come first, then more generic cases.
    const struct pci_match* match;
    const size_t num_matches;

    errno_t (*probe)(struct pci_device* dev, const struct pci_match* match);
};

errno_t pci_driver_register(struct pci_driver* driver);

INIT_EXTERN(pci_stage);

#ifdef __MODULE_NAME
#define PCI_LOG_NAME __MODULE_NAME
#else
#define PCI_LOG_NAME "pci"
#endif

#define pci_log_level(level, dev, message, ...) \
    log_write(level, PCI_LOG_NAME " " PCI_ADDR_FMT ": " message, PCI_ADDR_ARGS(&(dev)->address), ##__VA_ARGS__)

#define pci_log_debug(dev, message, ...) pci_log_level(LOG_DEBUG, dev, message, ##__VA_ARGS__)
#define pci_log(dev, message, ...)       pci_log_level(LOG_INFO, dev, message, ##__VA_ARGS__)
#define pci_log_warn(dev, message, ...)  pci_log_level(LOG_WARN, dev, message, ##__VA_ARGS__)
#define pci_log_err(dev, message, ...)   pci_log_level(LOG_ERR, dev, message, ##__VA_ARGS__)
