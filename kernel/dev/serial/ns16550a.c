// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ns16550a: " fmt

#include <dev/dt/fdt.h>
#include <lib/console.h>
#include <lib/cpu.h>
#include <lib/initgraph.h>
#include <lib/module.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <mem/mmio.h>
#include <mem/pmap.h>
#include <mem/vma.h>

struct ns16550a_device {
    struct console con;
    struct fdt_node node;
    void* regs;
};

constexpr mmio_reg8 DATA_REG = {.offset = 0, .endian = MMIO_LE};
constexpr mmio_reg8 LSR_REG = {.offset = 5, .endian = MMIO_LE};

constexpr uint8_t LSR_DATA_READY = 0x01;
constexpr uint8_t LSR_THR_EMPTY = 0x20;

static struct ns16550a_device* ns16550a_of(struct console* con) {
    return containerof(con, struct ns16550a_device, con);
}

static bool ns16550a_tx_ready(struct ns16550a_device* dev) {
    return (mmio_reg_read(dev->regs, LSR_REG) & LSR_THR_EMPTY) != 0;
}

static void ns16550a_write(struct console* con, const char* str, size_t size) {
    auto dev = ns16550a_of(con);
    for (size_t i = 0; i < size; i++) {
        while (!ns16550a_tx_ready(dev)) {
            cpu_pause();
        }

        mmio_reg_write(dev->regs, DATA_REG, str[i]);

        if (str[i] == '\n') {
            mmio_reg_write(dev->regs, DATA_REG, (uint8_t)'\r');
        }
    }
}

static errno_t ns16550a_fdt_probe(struct fdt_node node, const struct fdt_match* match) {
    log("got ns16550a, %s\n", fdt_node_name(node));

    struct ns16550a_device* dev = mem_alloc(sizeof(*dev), ALLOC_NOSLEEP);
    if (!dev)
        return ENOMEM;

    dev->con = (struct console){
        .name = fdt_node_name(dev->node),
        .priority = CONSOLE_PRIO_SERIAL,
        .write = ns16550a_write,
    };

    phys_t phys;
    size_t len;
    if (!fdt_node_reg(node, 0, &phys, &len)) {
        return EINVAL;
    }

    uintptr_t virt = vm_alloc(len);
    pmap_map(&kernel_vma.pmap, virt, phys, PTE_READ | PTE_WRITE, CACHE_MMIO);
    dev->regs = (void*)virt;

    console_register(&dev->con);
    return 0;
}

static struct fdt_match match_table[] = {
    {.compatible = "ns16550a", .data = 0},
};
MATCH_TABLE("fdt", match_table);

static struct fdt_driver ns16550a_fdt_driver = {
    .name = "ns16550a",
    .match = match_table,
    .num_matches = countof(match_table),
    .probe = ns16550a_fdt_probe,
};

static errno_t ns16550a_entry(struct module*) {
    fdt_driver_register(&ns16550a_fdt_driver);
    return 0;
}

errno_t ns16550a_exit(struct module*) {
    return 0;
}

MODULE_ENTRY(ns16550a_entry);
MODULE_EXIT(ns16550a_exit);

MODULE_DESC("NS16550A UART serial controller");
MODULE_AUTHOR("Marvin Friedrich");
MODULE_LICENSE("BSD-3-Clause");
MODULE_VERSION("1.0");
