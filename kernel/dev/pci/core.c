// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "pci: " fmt

#include <dev/pci/pci.h>
#include <lib/init.h>
#include <lib/irq.h>
#include <lib/module.h>
#include <lib/mutex.h>
#include <lib/print.h>
#include <lib/spin.h>
#include <lib/utils.h>
#include <lib/vec.h>
#include <mem/alloc.h>
#include <mem/mmio.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <string.h>

static struct spinlock access_lock = {};
static list_head(struct pci_access) accesses = {};

static mutex_t device_lock;
static vec(struct pci_device*) devices = {};
static vec(struct pci_driver*) drivers = {};

errno_t pci_access_register(struct pci_access* access) {
    spin_lock(&access_lock);
    list_insert_head(&accesses, access, link);
    spin_unlock(&access_lock);
    return 0;
}

static bool pci_access_decodes(const struct pci_access* access, const struct pci_address* addr) {
    return access->domain == addr->domain && addr->bus >= access->start_bus && addr->bus <= access->end_bus;
}

struct pci_access* pci_access_find(const struct pci_address* addr) {
    struct pci_access* found = nullptr;

    spin_lock(&access_lock);
    struct pci_access* access;
    list_foreach(access, &accesses, link) {
        if (!pci_access_decodes(access, addr))
            continue;
        if (found == nullptr || access->priority > found->priority)
            found = access;
    }
    spin_unlock(&access_lock);

    return found;
}

static uint32_t cfg_read32(struct pci_access* access, const struct pci_address* addr, uint16_t offset) {
    return access->read32(access, addr, offset & ~3);
}

static void cfg_write32(struct pci_access* access, const struct pci_address* addr, uint16_t offset, uint32_t value) {
    access->write32(access, addr, offset & ~3, value);
}

uint32_t pci_read32(struct pci_device* dev, uint16_t offset) {
    return cfg_read32(dev->access, &dev->address, offset);
}

uint16_t pci_read16(struct pci_device* dev, uint16_t offset) {
    return pci_read32(dev, offset) >> ((offset & 2) * 8);
}

uint8_t pci_read8(struct pci_device* dev, uint16_t offset) {
    return pci_read32(dev, offset) >> ((offset & 3) * 8);
}

void pci_write32(struct pci_device* dev, uint16_t offset, uint32_t value) {
    cfg_write32(dev->access, &dev->address, offset, value);
}

void pci_write16(struct pci_device* dev, uint16_t offset, uint16_t value) {
    uint32_t shift = (offset & 2) * 8;
    uint32_t reg = pci_read32(dev, offset);
    reg &= ~(0xffffu << shift);
    reg |= (uint32_t)value << shift;
    pci_write32(dev, offset, reg);
}

void pci_write8(struct pci_device* dev, uint16_t offset, uint8_t value) {
    uint32_t shift = (offset & 3) * 8;
    uint32_t reg = pci_read32(dev, offset);
    reg &= ~(0xffu << shift);
    reg |= (uint32_t)value << shift;
    pci_write32(dev, offset, reg);
}

errno_t pci_bar_get(struct pci_device* dev, size_t index, struct pci_bar* out) {
    if (index >= PCI_NUM_BARS)
        return EINVAL;

    uint16_t bar_offset = PCI_BAR0 + index * sizeof(uint32_t);
    uint16_t high_offset = bar_offset + sizeof(uint32_t);
    uint32_t bar = pci_read32(dev, bar_offset);

    const bool is_mmio = (bar & 0x1) == 0;
    const bool is_mmio64 = is_mmio && ((bar >> 1) & 0x3) == 0x2;
    const bool is_prefetchable = is_mmio && (bar & (1 << 3));
    const bool is_64bit_pair = is_mmio64 && index + 1 < PCI_NUM_BARS;

    uint16_t command = pci_read16(dev, PCI_COMMAND);
    pci_write16(dev, PCI_COMMAND, command & ~(PCI_COMMAND_IO | PCI_COMMAND_MEM));

    uint32_t high = is_64bit_pair ? pci_read32(dev, high_offset) : 0;

    pci_write32(dev, bar_offset, 0xffff'ffff);
    if (is_64bit_pair)
        pci_write32(dev, high_offset, 0xffff'ffff);

    uint32_t mask_low = pci_read32(dev, bar_offset);
    uint32_t mask_high = is_64bit_pair ? pci_read32(dev, high_offset) : 0;

    pci_write32(dev, bar_offset, bar);
    if (is_64bit_pair)
        pci_write32(dev, high_offset, high);

    pci_write16(dev, PCI_COMMAND, command);

    uint64_t addr_mask = is_mmio ? ~0xfull : ~0x3ull;
    uint64_t raw = (uint64_t)mask_high << 32 | mask_low;
    uint64_t masked = raw & addr_mask & (is_64bit_pair ? UINT64_MAX : UINT32_MAX);

    struct pci_bar result = {
        .size = masked == 0 ? 0 : (size_t)1 << __builtin_ctzll(masked),
        .prefetchable = is_prefetchable,
    };

    if (is_mmio && is_64bit_pair) {
        result.type = PCI_BAR_MMIO64;
        result.address = (phys_t)high << 32 | (bar & 0xffff'fff0);
    } else if (is_mmio) {
        result.type = PCI_BAR_MMIO32;
        result.address = bar & 0xffff'fff0;
    } else {
        result.type = PCI_BAR_IO;
        result.address = (uint16_t)(bar & 0xffff'fffc);
    }

    if (result.address == 0)
        return ENOENT;

    *out = result;
    return 0;
}

uint8_t pci_cap_next(struct pci_device* dev, uint8_t cur) {
    uint8_t ptr;
    if (cur == 0) {
        if (!(pci_read16(dev, PCI_STATUS) & PCI_STATUS_CAP_LIST))
            return 0;
        ptr = pci_read8(dev, PCI_CAP_PTR) & ~3;
    } else {
        ptr = (pci_read32(dev, cur) >> 8) & ~3;
    }

    if (ptr == 0)
        return 0;

    if ((pci_read32(dev, ptr) & 0xff) == 0xff)
        return 0;

    return ptr;
}

uint8_t pci_cap_find(struct pci_device* dev, uint8_t id) {
    for (uint8_t cap = pci_cap_next(dev, 0); cap != 0; cap = pci_cap_next(dev, cap)) {
        if (pci_read8(dev, cap) == id)
            return cap;
    }
    return 0;
}

constexpr uint32_t PCI_MSI_CTRL_ENABLE = 1 << 16;
constexpr uint32_t PCI_MSI_CTRL_MME = 0x7 << 20;
constexpr uint32_t PCI_MSI_CTRL_64BIT = 1 << 23;
constexpr uint32_t PCI_MSIX_CTRL_FUNCTION_MASK = 1 << 30;
constexpr uint32_t PCI_MSIX_CTRL_ENABLE = 1u << 31;

static void msix_set_state(struct pci_device* dev, uint8_t cap, bool status) {
    uint32_t ctrl = pci_read32(dev, cap);
    ctrl &= ~(PCI_MSIX_CTRL_ENABLE | PCI_MSIX_CTRL_FUNCTION_MASK);
    if (status)
        ctrl |= PCI_MSIX_CTRL_ENABLE;
    pci_write32(dev, cap, ctrl);
}

errno_t pci_setup_msix(struct pci_device* dev, struct irq_line** out) {
    // message address + data + vector control
    constexpr size_t ENTRY_SIZE = 8 + 4 + 4;

    const uint8_t cap = pci_cap_find(dev, PCI_CAP_MSIX);
    if (cap == 0)
        return ENODEV;

    const uint32_t table = pci_read32(dev, cap + 4);
    const uint8_t bir = table & 0x7;
    const size_t table_offset = table & ~0x7;

    pci_log(dev, "Using MSI-X for this device, table at BAR%hhu + %#zx\n", bir, table_offset);

    // Map the first MSI-X table entry (16 bytes).
    struct pci_bar bar;
    errno_t status = pci_bar_get(dev, bir, &bar);
    if (status)
        return ENODEV;
    if (bar.type != PCI_BAR_MMIO32 && bar.type != PCI_BAR_MMIO64)
        return EINVAL;

    const size_t pgsz = page_size();
    const phys_t table_phys = bar.address + table_offset;
    const phys_t page_base = ROUND_DOWN(table_phys, pgsz);
    const size_t page_offset = table_phys - page_base;
    const size_t map_len = ROUND_UP(page_offset + ENTRY_SIZE, pgsz);

    uintptr_t vaddr = vm_alloc(map_len);
    if (!vaddr)
        return ENOMEM;
    for (size_t i = 0; i < map_len; i += pgsz) {
        status = pmap_map(&kernel_vma.pmap, vaddr + i, page_base + i, PTE_READ | PTE_WRITE, CACHE_MMIO);
        if (status) {
            while (i) {
                i -= pgsz;
                pmap_unmap(&kernel_vma.pmap, vaddr + i);
            }
            vm_free(vaddr, map_len);
            return status;
        }
    }
    void* entry = (void*)(vaddr + page_offset);

    struct msi_line* msi;
    status = irq_alloc_msi(&msi);
    if (status)
        return status;
    irq_line_program(&msi->line, IRQ_TRIGGER_EDGE, IRQ_POLARITY_HIGH);

    // Write entry 0.
    const uint64_t msg_addr = msi->msg_addr(msi);
    const uint32_t msg_data = msi->msg_data(msi);
    mmio_write32(entry + 0x00, (uint32_t)msg_addr);         // Address lo
    mmio_write32(entry + 0x04, (uint32_t)(msg_addr >> 32)); // Address hi
    mmio_write32(entry + 0x08, msg_data);                   // Data
    mmio_write32(entry + 0x0C, 0);                          // Vector control

    // Enable MSI-X on the device.
    msix_set_state(dev, cap, true);

    *out = &msi->line;
    return 0;
}

errno_t pci_setup_msi(struct pci_device* dev, struct irq_line** out) {
    uint8_t cap = pci_cap_find(dev, PCI_CAP_MSI);
    if (cap == 0)
        return ENODEV;

    pci_log(dev, "Using MSI for this device\n");

    struct msi_line* msi;
    errno_t status = irq_alloc_msi(&msi);
    if (status)
        return status;
    irq_line_program(&msi->line, IRQ_TRIGGER_EDGE, IRQ_POLARITY_HIGH);

    const uint64_t addr = msi->msg_addr(msi);
    const uint16_t data = msi->msg_data(msi);

    uint32_t ctrl = pci_read32(dev, cap);
    pci_write32(dev, cap + 0x04, (uint32_t)addr);
    uint16_t data_off = 0x08;
    if (ctrl & PCI_MSI_CTRL_64BIT) {
        pci_write32(dev, cap + 0x08, (uint32_t)(addr >> 32));
        data_off = 0x0C;
    }
    pci_write32(dev, cap + data_off, data);

    ctrl &= ~PCI_MSI_CTRL_MME;
    ctrl |= PCI_MSI_CTRL_ENABLE;
    pci_write32(dev, cap, ctrl);

    *out = &msi->line;
    return 0;
}

errno_t pci_setup_irq(struct pci_device* dev, struct irq_line** out) {
    errno_t status = pci_setup_msix(dev, out);
    if (status == ENODEV)
        return pci_setup_msi(dev, out);
    return status;
}

static void pci_device_name(const struct pci_device* dev, char buf[static 18]) {
    ksnprintf(buf, 18, "pci-" PCI_ADDR_FMT, PCI_ADDR_ARGS(&dev->address));
}

static bool scan_function(struct pci_access* access, struct pci_address addr) {
    uint32_t reg0 = cfg_read32(access, &addr, PCI_VENDOR_ID);
    uint16_t vendor_id = reg0;
    uint16_t device_id = reg0 >> 16;

    if (vendor_id == 0xffff || vendor_id == 0)
        return false;
    if (device_id == 0xffff || device_id == 0)
        return false;

    struct pci_device* dev = mem_alloc(sizeof(*dev), 0);
    if (dev == nullptr) {
        log_err("Out of memory while scanning " PCI_ADDR_FMT "\n", PCI_ADDR_ARGS(&addr));
        return false;
    }

    dev->address = addr;
    dev->access = access;
    dev->dma_tag = &dma_default_tag;
    dev->id.vendor_id = vendor_id;
    dev->id.device_id = device_id;
    dev->class.prog_if = pci_read8(dev, PCI_PROG_IF);
    dev->class.subclass = pci_read8(dev, PCI_SUBCLASS);
    dev->class.class = pci_read8(dev, PCI_CLASS);
    dev->header_type = pci_read8(dev, PCI_HEADER_TYPE);

    if ((dev->header_type & PCI_HEADER_TYPE_MASK) == 0) {
        dev->id.subvendor_id = pci_read16(dev, PCI_SUBSYS_VENDOR_ID);
        dev->id.subdevice_id = pci_read16(dev, PCI_SUBSYS_ID);
    }

    pci_log(
        dev,
        "<id = %.4hx:%.4hx, class = %.2hhx:%.2hhx:%.2hhx>\n",
        dev->id.vendor_id,
        dev->id.device_id,
        dev->class.class,
        dev->class.subclass,
        dev->class.prog_if
    );

    if (vec_push(&devices, dev) != 0) {
        mem_free(dev, sizeof(*dev));
        return false;
    }

    return true;
}

static void scan_slot(struct pci_access* access, struct pci_address addr) {
    if (!scan_function(access, addr))
        return;

    if (!(vec_elem_get(&devices, vec_length(&devices) - 1)->header_type & PCI_HEADER_TYPE_MULTIFUNCTION))
        return;

    for (addr.function = 1; addr.function < PCI_NUM_FUNCTIONS; addr.function++)
        scan_function(access, addr);
}

static void format_match_id(char buf[static 5], uint16_t value) {
    if (value == PCI_ANY_ID)
        ksnprintf(buf, 5, "****");
    else
        ksnprintf(buf, 5, "%.4hx", value);
}

static void format_match_class(char buf[static 3], uint8_t value) {
    if (value == PCI_ANY_CLASS)
        ksnprintf(buf, 3, "**");
    else
        ksnprintf(buf, 3, "%.2hhx", value);
}

static void format_match(const struct pci_match* match, char* buf, size_t size) {
    char vendor[5], device[5], class[3], subclass[3], prog_if[3];
    format_match_id(vendor, match->vendor_id);
    format_match_id(device, match->device_id);
    format_match_class(class, match->class);
    format_match_class(subclass, match->subclass);
    format_match_class(prog_if, match->prog_if);
    ksnprintf(buf, size, "<id = %s:%s, class = %s.%s.%s>", vendor, device, class, subclass, prog_if);
}

static bool pci_match_device(const struct pci_match* m, const struct pci_device* dev) {
    return (m->vendor_id == PCI_ANY_ID || m->vendor_id == dev->id.vendor_id) &&
        (m->device_id == PCI_ANY_ID || m->device_id == dev->id.device_id) &&
        (m->class == PCI_ANY_CLASS || m->class == dev->class.class) &&
        (m->subclass == PCI_ANY_CLASS || m->subclass == dev->class.subclass) &&
        (m->prog_if == PCI_ANY_CLASS || m->prog_if == dev->class.prog_if);
}

// Returns the first entry that matches, or nullptr.
static const struct pci_match* pci_match_table(
    const struct pci_match* table,
    size_t num_matches,
    const struct pci_device* dev
) {
    for (size_t i = 0; i < num_matches; i++) {
        if (pci_match_device(&table[i], dev))
            return &table[i];
    }
    return nullptr;
}

errno_t pci_driver_register(struct pci_driver* driver) {
    if (driver->match == nullptr || driver->num_matches == 0) {
        log_warn("Driver \"%s\" does not define any matches, ignoring\n", driver->name);
        return 0;
    }

    mutex_lock(&device_lock);

    vec_foreach(&drivers, i) {
        if (strcmp(vec_elem_get(&drivers, i)->name, driver->name) == 0) {
            mutex_unlock(&device_lock);
            log_warn("Driver \"%s\" is already registered\n", driver->name);
            return EEXIST;
        }
    }

    errno_t status = vec_push(&drivers, driver);
    mutex_unlock(&device_lock);
    if (status != 0)
        return status;

    log("Registered PCI driver \"%s\" with %zu match%s\n",
        driver->name,
        driver->num_matches,
        driver->num_matches == 1 ? "" : "es");
    for (size_t i = 0; i < driver->num_matches; i++) {
        char rule[48];
        format_match(&driver->match[i], rule, sizeof(rule));
        log("  [%zu] = %s\n", i, rule);
    }

    for (size_t i = 0;; i++) {
        mutex_lock(&device_lock);
        if (i >= vec_length(&devices)) {
            mutex_unlock(&device_lock);
            break;
        }

        struct pci_device* dev = vec_elem_get(&devices, i);
        const struct pci_match* match = nullptr;
        if (dev->driver == nullptr) {
            match = pci_match_table(driver->match, driver->num_matches, dev);
            if (match != nullptr)
                dev->driver = driver;
        }
        mutex_unlock(&device_lock);

        if (match == nullptr)
            continue;

        pci_log(dev, "Matched driver \"%s\" on rule [%zu]\n", driver->name, (size_t)(match - driver->match));

        status = driver->probe(dev, match);
        if (status != 0) {
            pci_log_warn(dev, "%s failed to probe: %i\n", driver->name, status);
            mutex_lock(&device_lock);
            dev->driver = nullptr;
            mutex_unlock(&device_lock);
        }
    }

    return 0;
}

[[__init]]
static void pci_stage_init(struct boot_info*) {
    if (list_is_empty(&accesses)) {
        log_warn("No configuration space access method available\n");
        return;
    }

    log("Scanning PCI devices\n");

    mutex_lock(&device_lock);
    struct pci_access* access;
    list_foreach(access, &accesses, link) {
        for (uint32_t bus = access->start_bus; bus <= access->end_bus; bus++) {
            for (uint8_t slot = 0; slot < PCI_NUM_SLOTS; slot++) {
                struct pci_address addr = {
                    .domain = access->domain,
                    .bus = bus,
                    .device = slot,
                };

                if (pci_access_find(&addr) != access)
                    continue;

                scan_slot(access, addr);
            }
        }
    }
    mutex_unlock(&device_lock);

    log("Found %zu device(s)\n", vec_length(&devices));
}
INIT_TASK(pci_stage, "dev.pci", pci_stage_init);
INIT_DEPENDS(pci_stage, mem_stage);
