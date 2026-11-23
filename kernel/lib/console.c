// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "console: " fmt

#include <dev/tty/tty.h>
#include <lib/cmdline.h>
#include <lib/console.h>
#include <lib/init.h>
#include <lib/print.h>
#include <lib/spin.h>
#include <mem/alloc.h>
#include <string.h>

struct console_tty {
    struct tty tty;
    struct console* con;
};

static struct console* registered = nullptr;
static struct console* active = nullptr;
static struct spinlock console_lock;
static bool ttys_ready;
static uint32_t next_minor;

static const char* console_name;
static void console_option(const char* value) {
    console_name = value;
}
CMDLINE_OPTION("console", "Name of the console to use for /dev/console", console_option);

static void write_kernel(struct console* con, const char* buf, size_t len) {
    size_t start = 0;
    for (size_t i = 0; i < len; i++) {
        if (buf[i] != '\n')
            continue;
        con->write(con, buf + start, i - start);
        con->write(con, "\r\n", 2);
        start = i + 1;
    }
    if (start < len)
        con->write(con, buf + start, len - start);
}

void console_write(const char* buf, size_t len) {
    spin_lock(&console_lock);
    for (struct console* con = registered; con; con = con->next)
        write_kernel(con, buf, len);
    spin_unlock(&console_lock);
}

static void console_write_one(struct console* con, const char* buf, size_t len) {
    spin_lock(&console_lock);
    con->write(con, buf, len);
    spin_unlock(&console_lock);
}

static void console_replay_write(void* ctx, const char* buf, size_t len) {
    write_kernel(ctx, buf, len);
}

static struct console* console_of(struct tty* tty) {
    return containerof(tty, struct console_tty, tty)->con;
}

static errno_t console_tty_write(struct tty* tty, const char* buf, size_t len) {
    console_write_one(console_of(tty), buf, len);
    return 0;
}

static void console_tty_echo(struct tty* tty, const char* buf, size_t len) {
    console_write_one(console_of(tty), buf, len);
}

static const struct tty_ops console_tty_ops = {
    .write = console_tty_write,
    .echo = console_tty_echo,
};

static void set_tty(struct console* con, struct tty* tty) {
    spin_lock(&console_lock);
    con->tty = tty;

    // First check for the console named on the command line.
    // If it can't be found, use the console with the highest priority.
    bool activate;
    if (!active)
        activate = true;
    else if (console_name && !strcmp(active->name, console_name))
        activate = false;
    else if (console_name && !strcmp(con->name, console_name))
        activate = true;
    else
        activate = con->priority > active->priority;
    if (activate)
        active = con;
    spin_unlock(&console_lock);

    if (activate)
        log("Active console is now \"%s\"\n", con->name);
}

static void attach_tty(struct console* con) {
    if (con->get_tty) {
        struct tty* tty = con->get_tty(con);
        if (tty)
            set_tty(con, tty);
        return;
    }

    struct console_tty* ct = mem_alloc(sizeof(*ct), 0);
    if (!ct) {
        log_err("Unable to allocate a TTY for \"%s\"\n", con->name);
        return;
    }
    ct->con = con;

    errno_t status = tty_init(&ct->tty, con->name, &console_tty_ops, 4, 64 + next_minor++);
    if (status) {
        log_err("Unable to create a TTY for \"%s\" (%i)\n", con->name, status);
        mem_free(ct, sizeof(*ct));
        return;
    }

    if (con->cols && con->rows)
        ct->tty.winsize = (struct __winsize){.ws_row = con->rows, .ws_col = con->cols};

    status = tty_register(&ct->tty, 0666);
    if (status)
        log_err("Unable to create /dev/%s (%i)\n", con->name, status);

    set_tty(con, &ct->tty);
}

bool console_register(struct console* con) {
    if (con->init && !con->init(con))
        return false;

    con->write(con, "\ec", 2);
    log_replay(console_replay_write, con);
    con->tty = nullptr;
    con->next = nullptr;
    spin_lock(&console_lock);
    struct console** link = &registered;
    while (*link)
        link = &(*link)->next;
    *link = con;
    const bool ready = ttys_ready;
    spin_unlock(&console_lock);

    log("Registered console \"%s\"\n", con->name);

    if (ready)
        attach_tty(con);
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
    if (active == con) {
        active = nullptr;
        for (struct console* it = registered; it; it = it->next) {
            if (it->tty && (!active || it->priority > active->priority))
                active = it;
        }
    }
    spin_unlock(&console_lock);
}

static struct tty* get_tty_locked(struct console* con) {
    struct tty* tty = con ? con->tty : nullptr;
    if (tty)
        tty_ref(tty);
    return tty;
}

void console_input(struct console* con, const char* buf, size_t len) {
    spin_lock(&console_lock);
    struct tty* tty = get_tty_locked(con);
    spin_unlock(&console_lock);

    // Feed the input to the console's TTY, if it has one yet.
    if (!tty)
        return;

    tty_input(tty, buf, len);
    tty_unref(tty);
}

struct tty* console_get_tty() {
    spin_lock(&console_lock);
    struct tty* tty = get_tty_locked(active);
    spin_unlock(&console_lock);

    return tty;
}

errno_t console_activate_tty(struct tty* tty) {
    struct console* found = nullptr;

    spin_lock(&console_lock);
    for (struct console* con = registered; con; con = con->next) {
        if (con->tty == tty) {
            found = con;
            active = con;
            break;
        }
    }
    spin_unlock(&console_lock);

    if (!found)
        return ENOTTY;
    log("Active console is now \"%s\"\n", found->name);

    return 0;
}

INIT_TASK(console_stage, "lib.console");

static void console_tty_stage_init(struct boot_info* info) {
    spin_lock(&console_lock);
    ttys_ready = true;
    struct console* last = registered;
    while (last && last->next)
        last = last->next;
    spin_unlock(&console_lock);

    if (!last)
        return;
    for (struct console* con = registered;; con = con->next) {
        if (!con->tty)
            attach_tty(con);
        if (con == last)
            break;
    }
}

INIT_TASK(console_tty_stage, "lib.console.tty", console_tty_stage_init);
INIT_DEPENDS(console_tty_stage, console_stage, tty_stage);
