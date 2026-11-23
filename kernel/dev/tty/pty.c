// SPDX-License-Identifier: BSD-3-Clause

#include <dev/tty/tty.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <zinnia/termios.h>

constexpr size_t PTY_BUF_SIZE = 0x1000;
constexpr uint32_t PTY_SLAVE_MAJOR = 136;
constexpr size_t PTY_IO_CHUNK = 256;

struct pty {
    struct tty tty;
    uint32_t index;

    mutex_t lock;
    struct ringbuf master_buf;
    struct poll_event master_read_event;
    struct poll_event master_write_event;
    size_t slaves;
    bool slave_opened;
    bool unlocked;
    bool packet;
};

static _Atomic(uint32_t) next_index;
static struct chardev ptmx_dev;

static struct pty* pty_of(struct tty* tty) {
    return containerof(tty, struct pty, tty);
}

static errno_t write_int(uintptr_t arg, int value) {
    return usercopy_write((__user void*)arg, &value, sizeof(value)) ? 0 : EFAULT;
}

static errno_t read_int(uintptr_t arg, int* value) {
    return usercopy_read(value, (const __user void*)arg, sizeof(*value)) ? 0 : EFAULT;
}

static errno_t slave_write(struct tty* tty, const char* buf, size_t len) {
    struct pty* pty = pty_of(tty);

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &pty->master_write_event);

    errno_t status = 0;
    size_t done = 0;
    mutex_lock(&pty->lock);
    while (done < len) {
        const size_t written = ringbuf_write(&pty->master_buf, buf + done, len - done);
        if (written) {
            done += written;
            poll_event_notify(&pty->master_read_event);
            continue;
        }
        if (tty_is_hung_up(tty)) {
            status = EIO;
            break;
        }
        if (poll_table_sleep(&table, &pty->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&pty->lock);
    poll_table_fini(&table);
    return status;
}

static void slave_echo(struct tty* tty, const char* buf, size_t len) {
    struct pty* pty = pty_of(tty);

    mutex_lock(&pty->lock);
    const size_t written = ringbuf_write(&pty->master_buf, buf, len);
    mutex_unlock(&pty->lock);

    if (written)
        poll_event_notify(&pty->master_read_event);
}

static errno_t slave_open(struct tty* tty, struct vfs_file* file) {
    struct pty* pty = pty_of(tty);
    if (tty_is_hung_up(tty))
        return EIO;

    errno_t status = 0;
    mutex_lock(&pty->lock);
    if (pty->unlocked) {
        pty->slaves++;
        pty->slave_opened = true;
    } else {
        status = EIO;
    }
    mutex_unlock(&pty->lock);
    return status;
}

static void slave_close(struct tty* tty, struct vfs_file* file) {
    struct pty* pty = pty_of(tty);

    mutex_lock(&pty->lock);
    pty->slaves--;
    mutex_unlock(&pty->lock);

    poll_event_notify(&pty->master_read_event);
}

static void pty_destroy(struct tty* tty) {
    struct pty* pty = pty_of(tty);
    ringbuf_fini(&pty->master_buf);
    mem_free(pty, sizeof(*pty));
}

static const struct tty_ops pty_slave_ops = {
    .write = slave_write,
    .echo = slave_echo,
    .open = slave_open,
    .close = slave_close,
    .destroy = pty_destroy,
};

static bool slave_gone_locked(struct pty* pty) {
    return pty->slave_opened && pty->slaves == 0;
}

static errno_t master_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct pty* pty = file->priv;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &pty->master_read_event);

    errno_t status = 0;
    size_t done = 0;
    mutex_lock(&pty->lock);
    while (true) {
        if (ringbuf_len(&pty->master_buf)) {
            if (pty->packet) {
                const uint8_t header = TIOCPKT_DATA;
                status = iovec_iter_copy_to(buffer, &header, 1, &done);
                if (status)
                    break;
            }

            size_t read;
            status = ringbuf_read_iter(&pty->master_buf, buffer, iovec_iter_len(buffer), &read);
            done += read;
            poll_event_notify(&pty->master_write_event);
            break;
        }
        if (slave_gone_locked(pty)) {
            status = EIO;
            break;
        }
        if (atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK) {
            status = EAGAIN;
            break;
        }
        if (poll_table_sleep(&table, &pty->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&pty->lock);
    poll_table_fini(&table);

    *out_read = done;
    return done ? 0 : status;
}

static errno_t master_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct pty* pty = file->priv;

    char chunk[PTY_IO_CHUNK];
    size_t done = 0;
    errno_t status = 0;
    while (!status && iovec_iter_len(buffer)) {
        size_t len;
        status = iovec_iter_copy_from(buffer, chunk, sizeof(chunk), &len);
        if (!len)
            break;
        tty_input(&pty->tty, chunk, len);
        done += len;
    }

    *out_written = done;
    return done ? 0 : status;
}

static errno_t master_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    struct pty* pty = file->priv;
    errno_t status;
    int value;

    switch ((uint32_t)request) {
    case TIOCGPTN:
        return write_int(arg, pty->index);
    case TIOCSPTLCK:
        status = read_int(arg, &value);
        if (!status) {
            mutex_lock(&pty->lock);
            pty->unlocked = value == 0;
            mutex_unlock(&pty->lock);
        }
        return status;
    case TIOCGPTLCK:
        mutex_lock(&pty->lock);
        value = !pty->unlocked;
        mutex_unlock(&pty->lock);
        return write_int(arg, value);
    case TIOCPKT:
        status = read_int(arg, &value);
        if (!status) {
            mutex_lock(&pty->lock);
            pty->packet = value != 0;
            mutex_unlock(&pty->lock);
        }
        return status;
    case TIOCGPKT:
        mutex_lock(&pty->lock);
        value = pty->packet;
        mutex_unlock(&pty->lock);
        return write_int(arg, value);
    // TODO: TIOCSIG
    case FIONREAD:
        mutex_lock(&pty->lock);
        value = ringbuf_len(&pty->master_buf);
        mutex_unlock(&pty->lock);
        return write_int(arg, value);
    default:
        return tty_ioctl(&pty->tty, file, request, arg, out_result);
    }
}

static enum poll_flags master_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct pty* pty = file->priv;
    poll_wait(table, &pty->master_read_event);

    enum poll_flags revents = POLLOUT | POLLWRNORM;
    mutex_lock(&pty->lock);
    if (ringbuf_len(&pty->master_buf))
        revents |= POLLIN | POLLRDNORM;
    if (slave_gone_locked(pty))
        revents |= POLLHUP;
    mutex_unlock(&pty->lock);
    return revents;
}

static void master_release(struct vfs_file* file) {
    struct pty* pty = file->priv;

    tty_hangup(&pty->tty);
    poll_event_notify(&pty->master_read_event);
    poll_event_notify(&pty->master_write_event);
    // TODO: SIGHUP and SIGCONT to the foreground pgrp
    devtmpfs_remove(pty->tty.name);
    tty_unref(&pty->tty);
}

static const struct vfs_file_ops pty_master_ops = {
    .read = master_read,
    .write = master_write,
    .ioctl = master_ioctl,
    .poll = master_poll,
    .release = master_release,
};

static errno_t ptmx_open(struct chardev* dev, struct vfs_file* file) {
    struct pty* pty = mem_alloc(sizeof(*pty), 0);
    if (!pty)
        return ENOMEM;

    pty->index = atomic_fetch_add(&next_index, 1, ATOMIC_RELAXED);
    mutex_init(&pty->lock);
    poll_event_init(&pty->master_read_event);
    poll_event_init(&pty->master_write_event);

    char name[TTY_NAME_MAX];
    ksnprintf(name, sizeof(name), "pts/%u", pty->index);
    errno_t status = tty_init(&pty->tty, name, &pty_slave_ops, PTY_SLAVE_MAJOR, pty->index);
    if (status) {
        mem_free(pty, sizeof(*pty));
        return status;
    }

    status = ringbuf_init(&pty->master_buf, PTY_BUF_SIZE);
    if (!status)
        status = tty_register(&pty->tty, 0666);
    if (status) {
        tty_unref(&pty->tty);
        return status;
    }

    file->ops = &pty_master_ops;
    file->priv = pty;
    vfs_stream_open(file);
    return 0;
}

static const struct chardev_ops ptmx_dev_ops = {.open = ptmx_open};

static void pty_stage_init(struct boot_info* info) {
    errno_t status = devtmpfs_mkdir("pts", 0755);
    assert(status == 0, "Unable to create /dev/pts (%i)", status);

    chardev_init(&ptmx_dev, &ptmx_dev_ops, nullptr, 5, 2);
    status = devtmpfs_add_chardev("ptmx", &ptmx_dev, 0666);
    assert(status == 0, "Unable to create /dev/ptmx (%i)", status);
}

INIT_TASK(pty_stage, "dev.tty.pty", pty_stage_init);
INIT_DEPENDS(pty_stage, tty_stage);
