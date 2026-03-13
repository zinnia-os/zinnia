#include <kernel/cmdline.h>
#include <kernel/console.h>
#include <kernel/print.h>
#include <kernel/syscall.h>
#include <string.h>

extern struct console* const __ld_console_start[];
extern struct console* const __ld_console_end[];

struct console* active = nullptr;

void console_write(const char* buf, size_t len) {
    if (__likely(active))
        active->write(active, buf, len);
}

static void console_setup(const char* value) {
    if (!value)
        return;

    // Find a suitable boot console.
    struct console* const* cur = __ld_console_start;
    while (cur < __ld_console_end) {
        if (!strcmp(value, (*cur)->name)) {
            active = *cur;
        }
        cur++;
    }
}

CMDLINE_OPTION("console", console_setup);

void console_init() {
    // If we have no console set and at least one set, use that as a default.
    if (!active && &__ld_console_start[0] != &__ld_console_end[0])
        active = __ld_console_start[0];

    if (active) {
        if (active->init(active) == false)
            active = nullptr;
        else
            kprintf("Logging on console \"%s\"\n", active->name);
    }
}
