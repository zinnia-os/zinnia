// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "ns16550a: " fmt

#include <dev/dt/fdt.h>
#include <lib/console.h>
#include <lib/cpu.h>
#include <lib/irq.h>
#include <lib/module.h>
#include <lib/print.h>
#include <lib/ringbuf.h>
#include <lib/spin.h>
#include <mem/alloc.h>
#include <mem/mmio.h>
#include <mem/pmap.h>
#include <config.h>
#include <string.h>

#if CONFIG_ACPI
#include <dev/acpi/acpi.h>
#endif

#ifdef __x86_64__
#include <arch/x86_64/asm.h>
#endif

constexpr uint8_t REG_DATA = 0;
constexpr uint8_t REG_IER = 1;
constexpr uint8_t REG_FCR = 2;
constexpr uint8_t REG_LCR = 3;
constexpr uint8_t REG_MCR = 4;
constexpr uint8_t REG_LSR = 5;

constexpr uint8_t LSR_DATA_READY = 0x01;
constexpr uint8_t LSR_THR_EMPTY = 0x20;
constexpr uint8_t IER_RX_AVAILABLE = 0x01;

constexpr size_t RX_BUF_SIZE = 256;

enum ns16550a_flags : uintptr_t {
    NS16550A_PORT_IO = 1 << 0,
};

struct ns16550a_device {
    struct console con;
    void* regs;
    uint16_t port;
    bool pio;
    char name[8];

    struct irq_handler handler;
    struct spinlock rx_lock;
    struct ringbuf rx_buf;
};

static struct ns16550a_device* ns16550a_of(struct console* con) {
    return containerof(con, struct ns16550a_device, con);
}

static uint8_t ns16550a_read(struct ns16550a_device* dev, uint8_t reg) {
#ifdef __x86_64__
    if (dev->pio)
        return asm_inb(dev->port + reg);
#endif
    return mmio_read8((uint8_t*)dev->regs + reg);
}

static void ns16550a_write_reg(struct ns16550a_device* dev, uint8_t reg, uint8_t value) {
#ifdef __x86_64__
    if (dev->pio) {
        asm_outb(dev->port + reg, value);
        return;
    }
#endif
    mmio_write8((uint8_t*)dev->regs + reg, value);
}

static void ns16550a_write(struct console* con, const char* str, size_t size) {
    auto dev = ns16550a_of(con);
    for (size_t i = 0; i < size; i++) {
        while (!(ns16550a_read(dev, REG_LSR) & LSR_THR_EMPTY))
            cpu_pause();

        ns16550a_write_reg(dev, REG_DATA, str[i]);

        if (str[i] == '\n')
            ns16550a_write_reg(dev, REG_DATA, (uint8_t)'\r');
    }
}

static enum irq_status ns16550a_raise(struct irq_handler* self) {
    auto dev = containerof(self, struct ns16550a_device, handler);
    if (!(ns16550a_read(dev, REG_LSR) & LSR_DATA_READY))
        return IRQ_STATUS_IGNORED;

    spin_lock(&dev->rx_lock);
    while (ns16550a_read(dev, REG_LSR) & LSR_DATA_READY) {
        const uint8_t byte = ns16550a_read(dev, REG_DATA);
        ringbuf_write(&dev->rx_buf, &byte, 1);
    }
    spin_unlock(&dev->rx_lock);
    return IRQ_STATUS_WAKE_THREAD;
}

static void ns16550a_thread(struct irq_handler* self) {
    auto dev = containerof(self, struct ns16550a_device, handler);
    while (true) {
        char bytes[64];
        spin_lock(&dev->rx_lock);
        const size_t n = ringbuf_read(&dev->rx_buf, bytes, sizeof(bytes));
        spin_unlock(&dev->rx_lock);
        if (!n)
            break;
        console_input(&dev->con, bytes, n);
    }
}

static void ns16550a_attach_irq(
    struct ns16550a_device* dev,
    uint32_t gsi,
    enum irq_trigger_mode trigger,
    enum irq_polarity polarity
) {
    struct irq_line* line;
    errno_t status = irq_get_gsi(gsi, &line);
    if (status) {
        log_warn("%s: GSI %u not available (%i)\n", dev->name, gsi, status);
        return;
    }

    spin_init(&dev->rx_lock);
    status = ringbuf_init(&dev->rx_buf, RX_BUF_SIZE);
    if (status) {
        log_err("%s: Unable to allocate the receive buffer (%i)\n", dev->name, status);
        return;
    }

    dev->handler.raise = ns16550a_raise;
    dev->handler.thread = ns16550a_thread;
    status = irq_line_attach_task(line, &dev->handler, dev->name);
    if (status) {
        log_err("%s: Unable to attach the IRQ handler (%i)\n", dev->name, status);
        ringbuf_fini(&dev->rx_buf);
        return;
    }

    irq_line_program(line, trigger, polarity);
    ns16550a_write_reg(dev, REG_IER, IER_RX_AVAILABLE);
    line->unmask(line);
}

static void ns16550a_register(struct ns16550a_device* dev, const char* name) {
    dev->con = (struct console){
        .name = name,
        .priority = CONSOLE_PRIO_SERIAL,
        .write = ns16550a_write,
    };
    console_register(&dev->con);
}

static bool ns16550a_init(struct ns16550a_device* dev) {
    ns16550a_write_reg(dev, REG_IER, 0x00);
    ns16550a_write_reg(dev, REG_LCR, 0x80);
    ns16550a_write_reg(dev, REG_DATA, 0x01);
    ns16550a_write_reg(dev, REG_IER, 0x00);
    ns16550a_write_reg(dev, REG_LCR, 0x03);
    ns16550a_write_reg(dev, REG_FCR, 0xC7);
    ns16550a_write_reg(dev, REG_MCR, 0x0B);

    ns16550a_write_reg(dev, REG_MCR, 0x1E);
    ns16550a_write_reg(dev, REG_DATA, 0xAE);
    if (ns16550a_read(dev, REG_DATA) != 0xAE)
        return false;

    ns16550a_write_reg(dev, REG_MCR, 0x0F);
    return true;
}

static errno_t ns16550a_fdt_probe(struct fdt_node node, const struct fdt_match* match) {
    struct ns16550a_device* dev = mem_alloc(sizeof(*dev), 0);
    if (!dev)
        return ENOMEM;

    errno_t status = fdt_node_map_reg(node, 0, &dev->regs, nullptr);
    if (status) {
        mem_free(dev, sizeof(*dev));
        return status;
    }

    ns16550a_register(dev, fdt_node_name(node));
    return 0;
}

static const struct fdt_match fdt_match_table[] = {
    {.compatible = "ns16550a", .data = 0},
};
MATCH_TABLE("fdt", fdt_match_table);

static struct fdt_driver ns16550a_fdt_driver = {
    .name = "ns16550a",
    .match = fdt_match_table,
    .num_matches = countof(fdt_match_table),
    .probe = ns16550a_fdt_probe,
};

#if CONFIG_ACPI
static errno_t ns16550a_acpi_probe(struct acpi_device* adev, const struct acpi_match* match) {
    struct ns16550a_device* dev = mem_alloc(sizeof(*dev), 0);
    if (!dev)
        return ENOMEM;

    // TODO: Don't store the actual ACPI name here. Might need refactoring for sysfs.
    char name[5];
    acpi_device_name(adev, name);
    strncpy(dev->name, name, sizeof(name) - 1);

    struct acpi_resource res;
    errno_t status;

    if (match->data & NS16550A_PORT_IO) {
#ifdef __x86_64__
        status = acpi_device_resource(adev, ACPI_RESOURCE_IO, 0, &res);
        dev->pio = !status;
        dev->port = status ? 0 : res.io.base;
#else
        // Port IO is only really a thing on x86.
        status = ENOTSUP;
#endif
    } else {
        status = acpi_device_resource(adev, ACPI_RESOURCE_MEM, 0, &res);
        if (!status)
            status = mmio_map(res.mem.base, res.mem.length, CACHE_MMIO, &dev->regs);
    }

    if (status) {
        mem_free(dev, sizeof(*dev));
        return status;
    }

    if (dev->pio && !ns16550a_init(dev)) {
        mem_free(dev, sizeof(*dev));
        return ENODEV;
    }

    ns16550a_register(dev, dev->name);

    if (acpi_device_resource(adev, ACPI_RESOURCE_IRQ, 0, &res) == 0)
        ns16550a_attach_irq(dev, res.irq.gsi, res.irq.trigger, res.irq.polarity);
    return 0;
}

static const struct acpi_match acpi_match_table[] = {
    {.hid = "PNP0501", .data = NS16550A_PORT_IO}, // x86_64 COM1
};
MATCH_TABLE("acpi", acpi_match_table);

static struct acpi_driver ns16550a_acpi_driver = {
    .name = "ns16550a",
    .match = acpi_match_table,
    .num_matches = countof(acpi_match_table),
    .probe = ns16550a_acpi_probe,
};
#endif

static errno_t ns16550a_entry(struct module*) {
    errno_t status = fdt_driver_register(&ns16550a_fdt_driver);
    if (status && status != ENODEV)
        return status;
#if CONFIG_ACPI
    status = acpi_driver_register(&ns16550a_acpi_driver);
    if (status && status != ENODEV)
        return status;
#endif
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
