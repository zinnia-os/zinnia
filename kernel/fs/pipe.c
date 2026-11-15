// SPDX-License-Identifier: BSD-3-Clause

#include <fs/pipe.h>
#include <fs/poll.h>
#include <fs/private.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <lib/percpu.h>
#include <lib/ringbuf.h>
#include <lib/sched.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <zinnia/ioctls.h>

constexpr size_t PIPE_CAPACITY = 0x10000;

struct pipe {
    mutex_t lock;
    struct ringbuf buffer;
    size_t readers;
    size_t writers;
    uint64_t read_opens;
    uint64_t write_opens;
    struct poll_event read_event;
    struct poll_event write_event;
    struct poll_event open_event;
};

static const struct vfs_file_ops pipe_file_ops;

static errno_t pipe_new(struct pipe** out) {
    struct pipe* pipe = mem_alloc(sizeof(*pipe), 0);
    if (!pipe)
        return ENOMEM;

    errno_t status = ringbuf_init(&pipe->buffer, PIPE_CAPACITY);
    if (status) {
        mem_free(pipe, sizeof(*pipe));
        return status;
    }

    mutex_init(&pipe->lock);
    poll_event_init(&pipe->read_event);
    poll_event_init(&pipe->write_event);
    poll_event_init(&pipe->open_event);
    *out = pipe;
    return 0;
}

void pipe_destroy(struct pipe* pipe) {
    ringbuf_fini(&pipe->buffer);
    mem_free(pipe, sizeof(*pipe));
}

static bool file_readable(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_RDONLY;
}

static bool file_writable(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_WRONLY;
}

static bool file_nonblocking(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK;
}

static void notify_all(struct pipe* pipe) {
    poll_event_notify(&pipe->read_event);
    poll_event_notify(&pipe->write_event);
    poll_event_notify(&pipe->open_event);
}

static uint64_t attach_locked(struct pipe* pipe, struct vfs_file* file) {
    file->ops = &pipe_file_ops;
    file->priv = pipe;
    vfs_stream_open(file);

    if (file_readable(file)) {
        pipe->readers++;
        pipe->read_opens++;
    }
    if (file_writable(file)) {
        pipe->writers++;
        pipe->write_opens++;
    }
    return file_readable(file) ? pipe->write_opens : pipe->read_opens;
}

static void pipe_release(struct vfs_file* file) {
    struct pipe* pipe = file->priv;

    mutex_lock(&pipe->lock);
    if (file_readable(file))
        pipe->readers--;
    if (file_writable(file))
        pipe->writers--;
    mutex_unlock(&pipe->lock);

    notify_all(pipe);
}

static errno_t pipe_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct pipe* pipe = file->priv;
    const size_t want = iovec_iter_len(buffer);

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &pipe->read_event);

    errno_t status = 0;
    size_t read = 0;
    mutex_lock(&pipe->lock);
    while (true) {
        if (ringbuf_len(&pipe->buffer)) {
            status = ringbuf_read_iter(&pipe->buffer, buffer, want, &read);
            if (read)
                poll_event_notify(&pipe->write_event);
            break;
        }
        if (pipe->writers == 0)
            break;
        if (file_nonblocking(file)) {
            status = EAGAIN;
            break;
        }
        if (poll_table_sleep(&table, &pipe->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&pipe->lock);
    poll_table_fini(&table);

    *out_read = read;
    return status;
}

static errno_t pipe_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct pipe* pipe = file->priv;
    const size_t total = iovec_iter_len(buffer);
    const bool atomic = total <= __PIPE_BUF;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &pipe->write_event);

    errno_t status = 0;
    size_t done = 0;
    mutex_lock(&pipe->lock);
    while (done < total) {
        if (pipe->readers == 0) {
            status = EPIPE;
            break;
        }

        const size_t remaining = total - done;
        const size_t space = ringbuf_space(&pipe->buffer);
        if (space && (!atomic || space >= remaining)) {
            size_t written;
            status = ringbuf_write_iter(&pipe->buffer, buffer, remaining, &written);
            done += written;
            if (written)
                poll_event_notify(&pipe->read_event);
            if (status)
                break;
            continue;
        }

        if (file_nonblocking(file)) {
            status = EAGAIN;
            break;
        }
        if (poll_table_sleep(&table, &pipe->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&pipe->lock);
    poll_table_fini(&table);

    *out_written = done;
    return done ? 0 : status;
}

static errno_t pipe_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    struct pipe* pipe = file->priv;

    switch ((uint32_t)request) {
    case __FIONREAD: {
        mutex_lock(&pipe->lock);
        int len = ringbuf_len(&pipe->buffer);
        mutex_unlock(&pipe->lock);
        return usercopy_write((__user void*)arg, &len, sizeof(len)) ? 0 : EFAULT;
    }
    default:
        return ENOTTY;
    }
}

static enum poll_flags pipe_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct pipe* pipe = file->priv;

    if (file_readable(file))
        poll_wait(table, &pipe->read_event);
    if (file_writable(file))
        poll_wait(table, &pipe->write_event);

    enum poll_flags revents = 0;
    mutex_lock(&pipe->lock);
    if (file_readable(file)) {
        if (ringbuf_len(&pipe->buffer))
            revents |= POLLIN | POLLRDNORM;
        if (pipe->writers == 0)
            revents |= POLLHUP;
    }
    if (file_writable(file)) {
        if (ringbuf_space(&pipe->buffer) >= __PIPE_BUF)
            revents |= POLLOUT | POLLWRNORM;
        if (pipe->readers == 0)
            revents |= POLLERR;
    }
    mutex_unlock(&pipe->lock);
    return revents;
}

static const struct vfs_file_ops pipe_file_ops = {
    .read = pipe_read,
    .write = pipe_write,
    .ioctl = pipe_ioctl,
    .poll = pipe_poll,
    .release = pipe_release,
};

errno_t pipe_create(
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out_read,
    struct vfs_file** out_write
) {
    struct vfs_node* node;
    errno_t status = vfs_create_anon_inode(&pipe_file_ops, VNODE_FIFO, 0600, identity, &node);
    if (status)
        return status;

    status = pipe_new(&node->pipe);
    if (status) {
        vfs_node_unref(node);
        return status;
    }

    struct pipe* pipe = node->pipe;
    const enum open_flags extra = flags & O_NONBLOCK;
    struct vfs_file* read_end;
    struct vfs_file* write_end;
    status = vfs_file_new(node, nullptr, O_RDONLY | extra, &read_end);
    if (!status) {
        mutex_lock(&pipe->lock);
        attach_locked(pipe, read_end);
        mutex_unlock(&pipe->lock);

        status = vfs_file_new(node, nullptr, O_WRONLY | extra, &write_end);
        if (status)
            vfs_file_unref(read_end);
    }
    if (!status) {
        mutex_lock(&pipe->lock);
        attach_locked(pipe, write_end);
        mutex_unlock(&pipe->lock);
    }
    vfs_node_unref(node);
    if (status)
        return status;

    *out_read = read_end;
    *out_write = write_end;
    return 0;
}

errno_t vfs_fifo_open(struct vfs_node* node, struct vfs_file* file) {
    const bool readable = file_readable(file);
    const bool writable = file_writable(file);
    const bool nonblock = atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK;
    if (!readable && !writable)
        return EINVAL;

    errno_t status = 0;
    mutex_lock(&node->lock);
    if (!node->pipe)
        status = pipe_new(&node->pipe);
    struct pipe* pipe = node->pipe;
    mutex_unlock(&node->lock);
    if (status)
        return status;

    mutex_lock(&pipe->lock);
    if (writable && !readable && nonblock && pipe->readers == 0) {
        mutex_unlock(&pipe->lock);
        return ENXIO;
    }
    const uint64_t peer_opens = attach_locked(pipe, file);
    mutex_unlock(&pipe->lock);
    notify_all(pipe);

    if (nonblock || (readable && writable))
        return 0;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &pipe->open_event);

    mutex_lock(&pipe->lock);
    while (true) {
        const size_t peers = readable ? pipe->writers : pipe->readers;
        const uint64_t opens = readable ? pipe->write_opens : pipe->read_opens;
        if (peers || opens != peer_opens)
            break;
        if (poll_table_sleep(&table, &pipe->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&pipe->lock);
    poll_table_fini(&table);
    return status;
}
