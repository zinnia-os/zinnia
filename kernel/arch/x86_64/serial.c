#include <kernel/console.h>
#include <stdint.h>
#include <x86_64/asm.h>

constexpr uint16_t COM1_BASE = 0x3F8;

static bool is_ready() {
    return asm_inb(COM1_BASE + 5) & 0x20;
}

static void serial_write(struct console* con, const char* buf, size_t count) {
    for (size_t i = 0; i < count; i++) {
        while (!is_ready())
            asm volatile("pause");

        asm_outb(COM1_BASE, buf[i]);
    }
}

bool serial_setup(struct console* con) {
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

    return true;
}

static struct console serial_con = {
    .name = "com1",
    .write = serial_write,
    .init = serial_setup,
};
DEFINE_CONSOLE(&serial_con);
