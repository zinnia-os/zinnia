// SPDX-License-Identifier: BSD-3-Clause

#include <dev/tty/tty.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/console.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/usercopy.h>
#include <string.h>
#include <zinnia/termios.h>

constexpr size_t TTY_IO_CHUNK = 128;

struct tty_out {
    struct tty* tty;
    char buf[64];
    size_t len;
    bool wake;
};

static const struct vfs_file_ops tty_file_ops;

static struct chardev console_dev;

static struct process* current_process() {
    return percpu_get()->sched.current->process;
}

static bool is_cc(const struct __termios* t, size_t idx, char c) {
    return t->c_cc[idx] != 0 && t->c_cc[idx] == (__cc_t)c;
}

static bool is_eol(const struct __termios* t, char c) {
    return c == '\n' || is_cc(t, __VEOL, c) || is_cc(t, __VEOL2, c);
}

static void out_flush(struct tty_out* out) {
    if (!out->len)
        return;
    if (out->tty->ops->echo)
        out->tty->ops->echo(out->tty, out->buf, out->len);
    else
        out->tty->ops->write(out->tty, out->buf, out->len);
    out->len = 0;
}

static void out_put_raw(struct tty_out* out, char c) {
    if (out->len == sizeof(out->buf))
        out_flush(out);
    out->buf[out->len++] = c;
}

static void out_put(struct tty_out* out, char c) {
    const __tcflag_t oflag = out->tty->termios.c_oflag;
    if (c == '\n' && (oflag & __OPOST) && (oflag & __ONLCR))
        out_put_raw(out, '\r');
    out_put_raw(out, c);
}

static void out_puts(struct tty_out* out, const char* str) {
    while (*str)
        out_put(out, *str++);
}

static void push_line(struct tty* tty, struct tty_out* out) {
    ringbuf_write(&tty->read_buf, tty->canon_buf, tty->canon_len);
    tty->canon_len = 0;
    out->wake = true;
}

void tty_input(struct tty* tty, const char* buf, size_t len) {
    struct tty_out out = {.tty = tty};
    const struct __termios* t = &tty->termios;

    mutex_lock(&tty->lock);
    for (size_t i = 0; i < len; i++) {
        char c = buf[i];

        // Translate CR and NL according to the input flags.
        if (c == '\r') {
            if (t->c_iflag & __IGNCR)
                continue;
            if (t->c_iflag & __ICRNL)
                c = '\n';
        } else if (c == '\n' && (t->c_iflag & __INLCR)) {
            c = '\r';
        }

        // TODO: ISIG signal delivery to the foreground pgrp.

        if (!(t->c_lflag & __ICANON)) {
            // Pass the byte straight through in raw mode.
            ringbuf_write(&tty->read_buf, &c, 1);
            if (t->c_lflag & __ECHO)
                out_put(&out, c);
            out.wake = true;
            continue;
        }

        // Apply line editing in canonical mode.
        const bool echo = t->c_lflag & __ECHO;
        if (is_cc(t, __VERASE, c)) {
            if (tty->canon_len) {
                tty->canon_len--;
                if (echo && (t->c_lflag & __ECHOE))
                    out_puts(&out, "\b \b");
            }
            continue;
        }

        if (is_cc(t, __VKILL, c)) {
            if (echo) {
                for (size_t j = 0; j < tty->canon_len; j++)
                    out_puts(&out, "\b \b");
            }
            tty->canon_len = 0;
            continue;
        }

        if (is_cc(t, __VEOF, c)) {
            if (tty->canon_len) {
                push_line(tty, &out);
            } else {
                tty->eof_pending++;
                out.wake = true;
            }
            continue;
        }

        if (tty->canon_len < TTY_BUF_SIZE)
            tty->canon_buf[tty->canon_len++] = c;

        if (echo || (c == '\n' && (t->c_lflag & __ECHONL)))
            out_put(&out, c);

        if (is_eol(t, c))
            push_line(tty, &out);
    }
    out_flush(&out);
    mutex_unlock(&tty->lock);

    if (out.wake)
        poll_event_notify(&tty->read_event);
}

static bool file_nonblocking(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK;
}

static bool canon_readable(struct tty* tty) {
    return ringbuf_len(&tty->read_buf) || tty->eof_pending;
}

static errno_t tty_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct tty* tty = file->priv;
    // TODO: SIGTTIN job control gate for background process groups.

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &tty->read_event);

    size_t done = 0;
    errno_t status = 0;
    mutex_lock(&tty->lock);
    if (tty->termios.c_lflag & __ICANON) {
        // Wait for a complete line or an EOF.
        while (!status && !canon_readable(tty)) {
            if (file_nonblocking(file))
                status = EAGAIN;
            else if (poll_table_sleep(&table, &tty->lock, nullptr) == EINTR)
                status = ERESTART;
        }

        if (!status && !ringbuf_len(&tty->read_buf)) {
            tty->eof_pending--;
        } else if (!status) {
            // Copy out at most one line.
            char chunk[TTY_IO_CHUNK];
            bool eol = false;
            while (!eol && iovec_iter_len(buffer) && ringbuf_len(&tty->read_buf)) {
                size_t len = 0;
                const size_t max = MIN(sizeof(chunk), iovec_iter_len(buffer));
                while (len < max && !eol && ringbuf_read(&tty->read_buf, &chunk[len], 1))
                    eol = is_eol(&tty->termios, chunk[len++]);

                size_t copied;
                status = iovec_iter_copy_to(buffer, chunk, len, &copied);
                done += copied;
                if (status)
                    break;
            }
        }
    } else {
        // Read until VMIN bytes have arrived or the VTIME timer expires.
        const size_t vmin = tty->termios.c_cc[__VMIN];
        const uint64_t interval = (uint64_t)tty->termios.c_cc[__VTIME] * 100'000'000;
        const size_t target = vmin ? MIN(vmin, iovec_iter_len(buffer)) : 1;
        uint64_t deadline = vmin == 0 && interval ? clock_get_elapsed_ns() + interval : 0;

        while (true) {
            if (ringbuf_len(&tty->read_buf)) {
                size_t read;
                status = ringbuf_read_iter(&tty->read_buf, buffer, iovec_iter_len(buffer), &read);
                done += read;
                if (status || done >= target || !iovec_iter_len(buffer))
                    break;
                if (interval)
                    deadline = clock_get_elapsed_ns() + interval;
                continue;
            }

            if (vmin == 0 && interval == 0)
                break;
            if (file_nonblocking(file)) {
                status = EAGAIN;
                break;
            }
            if (deadline && clock_get_elapsed_ns() >= deadline)
                break;
            if (poll_table_sleep(&table, &tty->lock, deadline ? &deadline : nullptr) == EINTR) {
                status = ERESTART;
                break;
            }
        }
    }
    mutex_unlock(&tty->lock);
    poll_table_fini(&table);

    *out_read = done;
    return done ? 0 : status;
}

static errno_t tty_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct tty* tty = file->priv;
    // TODO: TOSTOP/SIGTTOU job control gate and hangup check.

    char in[TTY_IO_CHUNK];
    char out[TTY_IO_CHUNK * 2];
    size_t done = 0;
    errno_t status = 0;
    while (!status && iovec_iter_len(buffer)) {
        size_t len;
        status = iovec_iter_copy_from(buffer, in, sizeof(in), &len);
        if (!len)
            break;

        mutex_lock(&tty->lock);
        const __tcflag_t oflag = tty->termios.c_oflag;
        mutex_unlock(&tty->lock);

        // Expand NL to CR-NL if ONLCR is set.
        const bool onlcr = (oflag & __OPOST) && (oflag & __ONLCR);
        size_t out_len = 0;
        for (size_t i = 0; i < len; i++) {
            if (onlcr && in[i] == '\n')
                out[out_len++] = '\r';
            out[out_len++] = in[i];
        }

        errno_t write_status = tty->ops->write(tty, out, out_len);
        if (write_status) {
            status = write_status;
            break;
        }
        done += len;
    }

    *out_written = done;
    return done ? 0 : status;
}

static enum poll_flags tty_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct tty* tty = file->priv;
    poll_wait(table, &tty->read_event);

    enum poll_flags revents = POLLOUT | POLLWRNORM;
    mutex_lock(&tty->lock);
    const bool readable = (tty->termios.c_lflag & __ICANON) ? canon_readable(tty) : ringbuf_len(&tty->read_buf) != 0;
    mutex_unlock(&tty->lock);
    if (readable)
        revents |= POLLIN | POLLRDNORM;
    return revents;
}

static void flush_input_locked(struct tty* tty) {
    ringbuf_clear(&tty->read_buf);
    tty->canon_len = 0;
    tty->eof_pending = 0;
}

static errno_t write_int(uintptr_t arg, int value) {
    return usercopy_write((__user void*)arg, &value, sizeof(value)) ? 0 : EFAULT;
}

static errno_t set_termios(struct tty* tty, uintptr_t arg, bool flush) {
    struct __termios termios;
    if (!usercopy_read(&termios, (const __user void*)arg, sizeof(termios)))
        return EFAULT;

    mutex_lock(&tty->lock);
    if (flush)
        flush_input_locked(tty);
    if ((tty->termios.c_lflag & __ICANON) && !(termios.c_lflag & __ICANON) && tty->canon_len) {
        ringbuf_write(&tty->read_buf, tty->canon_buf, tty->canon_len);
        tty->canon_len = 0;
    }
    tty->termios = termios;
    mutex_unlock(&tty->lock);

    poll_event_notify(&tty->read_event);
    return 0;
}

void tty_set_winsize(struct tty* tty, const struct __winsize* winsize) {
    mutex_lock(&tty->lock);
    tty->winsize = *winsize;
    mutex_unlock(&tty->lock);
    // TODO: SIGWINCH to the foreground pgrp when the size changes.
}

errno_t tty_ioctl(struct tty* tty, struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    switch ((uint32_t)request) {
    case TCGETS: {
        mutex_lock(&tty->lock);
        struct __termios termios = tty->termios;
        mutex_unlock(&tty->lock);
        return usercopy_write((__user void*)arg, &termios, sizeof(termios)) ? 0 : EFAULT;
    }
    case TCSETS:
    case TCSETSW:
        return set_termios(tty, arg, false);
    case TCSETSF:
        return set_termios(tty, arg, true);
    case TCFLSH:
        if (arg != __TCIFLUSH && arg != __TCOFLUSH && arg != __TCIOFLUSH)
            return EINVAL;
        if (arg != __TCOFLUSH) {
            mutex_lock(&tty->lock);
            flush_input_locked(tty);
            mutex_unlock(&tty->lock);
        }
        return 0;
    case TIOCGWINSZ: {
        mutex_lock(&tty->lock);
        struct __winsize winsize = tty->winsize;
        mutex_unlock(&tty->lock);
        return usercopy_write((__user void*)arg, &winsize, sizeof(winsize)) ? 0 : EFAULT;
    }
    case TIOCSWINSZ: {
        struct __winsize winsize;
        if (!usercopy_read(&winsize, (const __user void*)arg, sizeof(winsize)))
            return EFAULT;
        tty_set_winsize(tty, &winsize);
        return 0;
    }
    case FIONREAD: {
        mutex_lock(&tty->lock);
        int len = ringbuf_len(&tty->read_buf);
        mutex_unlock(&tty->lock);
        return write_int(arg, len);
    }
    case TIOCGNAME: {
        if (!file->absolute_path)
            return ENOENT;
        const size_t len = strlen(file->absolute_path) + 1;
        return usercopy_write((__user void*)arg, file->absolute_path, len) ? 0 : EFAULT;
    }
    // TODO: TIOCGPGRP, TIOCSPGRP, TIOCGSID, TIOCSCTTY, TIOCNOTTY
    case TIOCCONS:
        if (current_process()->identity.euid != 0)
            return EPERM;
        return console_activate_tty(tty);
    default:
        return tty->ops->ioctl ? tty->ops->ioctl(tty, file, request, arg, out_result) : ENOTTY;
    }
}

static errno_t tty_file_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    return tty_ioctl(file->priv, file, request, arg, out_result);
}

static void tty_release(struct vfs_file* file) {
    tty_unref(file->priv);
}

static const struct vfs_file_ops tty_file_ops = {
    .read = tty_read,
    .write = tty_write,
    .ioctl = tty_file_ioctl,
    .poll = tty_poll,
    .release = tty_release,
};

static errno_t attach_file(struct tty* tty, struct vfs_file* file) {
    tty_ref(tty);
    file->ops = &tty_file_ops;
    file->priv = tty;
    vfs_stream_open(file);
    return 0;
}

static errno_t tty_dev_open(struct chardev* dev, struct vfs_file* file) {
    return attach_file(containerof(dev, struct tty, dev), file);
}

static void tty_dev_destroy(struct chardev* dev) {
    struct tty* tty = containerof(dev, struct tty, dev);
    ringbuf_fini(&tty->read_buf);
    if (tty->ops->destroy)
        tty->ops->destroy(tty);
}

static const struct chardev_ops tty_dev_ops = {
    .open = tty_dev_open,
    .destroy = tty_dev_destroy,
};

errno_t tty_init(struct tty* tty, const char* name, const struct tty_ops* ops, uint32_t major, uint32_t minor) {
    errno_t status = ringbuf_init(&tty->read_buf, TTY_BUF_SIZE);
    if (status)
        return status;

    chardev_init(&tty->dev, &tty_dev_ops, &tty_file_ops, major, minor);
    strncpy(tty->name, name, sizeof(tty->name) - 1);
    tty->ops = ops;
    mutex_init(&tty->lock);
    // Start with the standard termios defaults.
    struct __termios* t = &tty->termios;
    memset(t, 0, sizeof(*t));
    t->c_iflag = __ICRNL | __IXON;
    t->c_oflag = __OPOST | __ONLCR;
    t->c_cflag = __CS8 | __CREAD;
    t->c_lflag = __ISIG | __ICANON | __ECHO | __ECHOE | __ECHOK | __ECHONL | __IEXTEN;

    t->c_cc[__VINTR] = 0x03;
    t->c_cc[__VQUIT] = 0x1C;
    t->c_cc[__VERASE] = 0x7F;
    t->c_cc[__VKILL] = 0x15;
    t->c_cc[__VEOF] = 0x04;
    t->c_cc[__VTIME] = 0;
    t->c_cc[__VMIN] = 1;
    t->c_cc[__VSTART] = 0x11;
    t->c_cc[__VSTOP] = 0x13;
    t->c_cc[__VSUSP] = 0x1A;
    t->c_cc[__VREPRINT] = 0x12;
    t->c_cc[__VDISCARD] = 0x0F;
    t->c_cc[__VWERASE] = 0x17;
    t->c_cc[__VLNEXT] = 0x16;
    tty->winsize = (struct __winsize){.ws_row = 25, .ws_col = 80};
    tty->canon_len = 0;
    tty->eof_pending = 0;
    poll_event_init(&tty->read_event);
    return 0;
}

void tty_ref(struct tty* tty) {
    chardev_ref(&tty->dev);
}

void tty_unref(struct tty* tty) {
    chardev_unref(&tty->dev);
}

errno_t tty_register(struct tty* tty, mode_t mode) {
    return devtmpfs_add_chardev(tty->name, &tty->dev, mode);
}

static errno_t console_open(struct chardev* dev, struct vfs_file* file) {
    struct tty* tty = console_get_tty();
    if (!tty)
        return ENXIO;
    errno_t status = attach_file(tty, file);
    tty_unref(tty);
    return status;
}

static const struct chardev_ops console_dev_ops = {.open = console_open};

static void tty_stage_init(struct boot_info* info) {
    // TODO: /dev/tty
    chardev_init(&console_dev, &console_dev_ops, nullptr, 5, 1);
    errno_t status = devtmpfs_add_chardev("console", &console_dev, 0600);
    assert(status == 0, "Unable to create /dev/console (%i)", status);
}

INIT_TASK(tty_stage, "dev.tty", tty_stage_init);
INIT_DEPENDS(tty_stage, devtmpfs_stage);
