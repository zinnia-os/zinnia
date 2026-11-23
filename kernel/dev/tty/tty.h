// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <dev/char/chardev.h>
#include <fs/poll.h>
#include <lib/atomic.h>
#include <lib/initgraph.h>
#include <lib/mutex.h>
#include <lib/ringbuf.h>
#include <lib/signal.h>
#include <lib/types.h>
#include <zinnia/termios.h>

constexpr size_t TTY_BUF_SIZE = 4096;
constexpr size_t TTY_NAME_MAX = 32;

struct tty;

struct tty_ops {
    errno_t (*write)(struct tty* tty, const char* buf, size_t len);
    void (*echo)(struct tty* tty, const char* buf, size_t len);
    errno_t (*open)(struct tty* tty, struct vfs_file* file);
    void (*close)(struct tty* tty, struct vfs_file* file);
    errno_t (*ioctl)(struct tty* tty, struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result);
    void (*destroy)(struct tty* tty);
};

struct tty {
    struct chardev dev;
    char name[TTY_NAME_MAX];
    const struct tty_ops* ops;

    mutex_t lock;
    struct __termios termios;
    struct __winsize winsize;
    struct ringbuf read_buf;
    char canon_buf[TTY_BUF_SIZE];
    size_t canon_len;
    size_t eof_pending;
    struct poll_event read_event;
    _Atomic(bool) hung_up;

    pid_t pgrp;
    pid_t session;
};

errno_t tty_init(struct tty* tty, const char* name, const struct tty_ops* ops, uint32_t major, uint32_t minor);
void tty_ref(struct tty* tty);
void tty_unref(struct tty* tty);
errno_t tty_register(struct tty* tty, mode_t mode);

void tty_input(struct tty* tty, const char* buf, size_t len);
void tty_hangup(struct tty* tty);
void tty_set_winsize(struct tty* tty, const struct __winsize* winsize);
void tty_signal_foreground(struct tty* tty, signal_t sig);
bool tty_is_hung_up(struct tty* tty);

errno_t tty_ioctl(struct tty* tty, struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result);
void tty_detach_session_locked(struct tty* tty);

INIT_EXTERN(tty_stage);
