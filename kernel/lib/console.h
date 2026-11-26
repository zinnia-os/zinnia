// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/utils.h>
#include <stddef.h>
#include <stdint.h>

struct tty;

enum console_prio : uint8_t {
    CONSOLE_PRIO_SERIAL,
    CONSOLE_PRIO_DISPLAY,
};

struct console {
    const char* name;
    enum console_prio priority;
    uint16_t cols;
    uint16_t rows;

    // Writes to the console.
    void (*write)(struct console* con, const char* buf, size_t count);
    struct tty* (*get_tty)(struct console* con);

    struct tty* tty;
    struct console* next;
};

INIT_EXTERN(console_stage);
INIT_EXTERN(console_tty_stage);

// Registers a console.
void console_register(struct console* con);

// Unregisters a regular console.
void console_unregister(struct console* con);

// Writes a message to a console.
void console_write(const char* buf, size_t len);

void console_input(struct console* con, const char* buf, size_t len);
struct tty* console_get_tty();
errno_t console_activate_tty(struct tty* tty);
