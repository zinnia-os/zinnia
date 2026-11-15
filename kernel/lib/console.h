// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/initgraph.h>
#include <lib/utils.h>
#include <stddef.h>

struct console {
    const char* name;
    // Initializes the console.
    bool (*init)(struct console* con);
    // Writes to the console.
    void (*write)(struct console* con, const char* buf, size_t count);
    struct console* next;
};

INIT_EXTERN(console_stage);

// Registers a console.
bool console_register(struct console* con);

// Unregisters a regular console.
void console_unregister(struct console* con);

// Writes a message to a console.
void console_write(const char* buf, size_t len);
