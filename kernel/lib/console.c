// SPDX-License-Identifier: BSD-3-Clause

#include <lib/console.h>
#include <lib/init.h>
#include <lib/print.h>
#include <lib/spin.h>

static struct console* registered = nullptr;
static struct spinlock console_lock;

void console_write(const char* buf, size_t len) {
    spin_lock(&console_lock);
    for (struct console* con = registered; con; con = con->next)
        con->write(con, buf, len);
    spin_unlock(&console_lock);
}

static void console_replay_write(void* ctx, const char* buf, size_t len) {
    struct console* con = ctx;
    con->write(con, buf, len);
}

bool console_register(struct console* con) {
    if (!con->init(con))
        return false;

    con->write(con, "\ec", 2);
    log_replay(console_replay_write, con);
    spin_lock(&console_lock);
    con->next = registered;
    registered = con;
    spin_unlock(&console_lock);

    log("console: Registered console \"%s\"\n", con->name);
    return true;
}

void console_unregister(struct console* con) {
    spin_lock(&console_lock);
    for (struct console** link = &registered; *link; link = &(*link)->next) {
        if (*link == con) {
            *link = con->next;
            break;
        }
    }
    spin_unlock(&console_lock);
}

INIT_TASK(console_stage, "lib.console");
