#pragma once

#include <kernel/common.h>
#include <kernel/compiler.h>
#include <stddef.h>

struct console {
    const char* name;
    void* private;

    // Initializes the console.
    bool (*init)(struct console* con);
    // Writes to the console.
    void (*write)(struct console* con, const char* buf, size_t count);
};

// Initializes a console for logging.
void console_init();

// Writes a message to a console.
void console_write(const char* buf, size_t len);

// Defines a new early console which can be used to log early boot messages.
// An early console must not make any heap allocations.
#define DEFINE_CONSOLE(con) \
    [[__used, __section(".console")]] \
    static struct console* UNIQUE_IDENT(console) = (con)
