// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "x86_64/serial: " fmt

#include <arch/x86_64/apic.h>
#include <arch/x86_64/asm.h>
#include <lib/cmdline.h>
#include <lib/console.h>
#include <lib/cpu.h>
#include <lib/init.h>
#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/print.h>
#include <lib/ringbuf.h>
#include <lib/sched.h>
#include <lib/spin.h>
#include <stdint.h>

constexpr uint16_t COM1_BASE = 0x3F8;
constexpr uint8_t COM1_ISA_IRQ = 4;
constexpr size_t COM1_BUF_SIZE = 256;

static bool serial_present;
static bool com1_enabled = true;
static void com1_option(bool opt) {
    com1_enabled = opt;
}
CMDLINE_OPTION("x86_64.com1", "Enable COM1 input", com1_option);

static struct irq_handler com1_handler;
static struct spinlock com1_lock;
static struct ringbuf com1_buf;

static bool is_ready() {
    return asm_inb(COM1_BASE + 5) & 0x20;
}

static bool is_data_ready() {
    return asm_inb(COM1_BASE + 5) & 0x01;
}

static void serial_write(struct console* con, const char* buf, size_t count) {
    for (size_t i = 0; i < count; i++) {
        while (!is_ready())
            arch_cpu_pause();

        asm_outb(COM1_BASE, buf[i]);
    }
}

static bool serial_setup() {
    asm_outb(COM1_BASE + 1, 0x00); // Disable interrupts
    asm_outb(COM1_BASE + 3, 0x80); // Enable DLAB (set baud rate divisor)
    asm_outb(COM1_BASE, 0x01);     // Set divisor low byte (115200 baud if 1)
    asm_outb(COM1_BASE + 1, 0x00); // Set divisor high byte
    asm_outb(COM1_BASE + 3, 0x03); // 8 bits, no parity, one stop bit (8n1)
    asm_outb(COM1_BASE + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
    asm_outb(COM1_BASE + 4, 0x0B); // IRQs enabled, RTS/DSR set

    asm_outb(COM1_BASE + 4, 0x1E); // Set to loopback mode.
    asm_outb(COM1_BASE, 0xAE);     // Send a test byte.

    // If we don't get the same value back, this serial port doesn't work.
    if (asm_inb(COM1_BASE) != 0xAE) {
        return false;
    }

    asm_outb(COM1_BASE + 4, 0x0F); // Disable loopback mode.

    serial_present = true;
    return true;
}

static struct console serial_con = {
    .name = "com1",
    .priority = CONSOLE_PRIO_SERIAL,
    .write = serial_write,
};

[[__init]]
static void serial_init(struct boot_info*) {
    if (serial_setup())
        console_register(&serial_con);
}
INIT_TASK(serial_stage, "arch.x86_64.serial", serial_init);
INIT_ENTAILS(serial_stage, console_stage);

static enum irq_status com1_raise(struct irq_handler* self) {
    if (!is_data_ready())
        return IRQ_STATUS_IGNORED;

    spin_lock(&com1_lock);
    while (is_data_ready()) {
        const uint8_t byte = asm_inb(COM1_BASE);
        ringbuf_write(&com1_buf, &byte, 1);
    }
    spin_unlock(&com1_lock);
    return IRQ_STATUS_WAKE_THREAD;
}

static void com1_thread(struct irq_handler* self) {
    while (true) {
        char bytes[64];
        spin_lock(&com1_lock);
        const size_t n = ringbuf_read(&com1_buf, bytes, sizeof(bytes));
        spin_unlock(&com1_lock);
        if (!n)
            break;
        console_input(&serial_con, bytes, n);
    }
}

static void com1_init(struct boot_info*) {
    if (!serial_present || !com1_enabled)
        return;

    uint32_t gsi = COM1_ISA_IRQ;
    enum irq_trigger_mode trigger = IRQ_TRIGGER_EDGE;
    enum irq_polarity polarity = IRQ_POLARITY_HIGH;
    arch_irq_isa_override(COM1_ISA_IRQ, &gsi, &trigger, &polarity);

    struct irq_line* line;
    errno_t status = arch_irq_get_gsi(gsi, &line);
    if (status) {
        log_warn("COM1 IRQ %u not available (%i)\n", COM1_ISA_IRQ, status);
        return;
    }

    spin_init(&com1_lock);
    status = ringbuf_init(&com1_buf, COM1_BUF_SIZE);
    if (status) {
        log_err("Unable to allocate the COM1 buffer (%i)\n", status);
        return;
    }

    com1_handler.raise = com1_raise;
    com1_handler.thread = com1_thread;
    status = irq_line_attach_task(line, &com1_handler, "com1");
    if (status) {
        log_err("Unable to attach the COM1 IRQ handler (%i)\n", status);
        ringbuf_fini(&com1_buf);
        return;
    }

    irq_line_program(line, trigger, polarity);
    asm_outb(COM1_BASE + 1, 0x01);
    line->unmask(line);
}
INIT_TASK(com1_stage, "arch.x86_64.com1", com1_init);
INIT_DEPENDS(com1_stage, serial_stage, ioapic_stage, sched_stage);
