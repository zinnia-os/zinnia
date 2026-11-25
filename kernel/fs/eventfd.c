// SPDX-License-Identifier: BSD-3-Clause

#include <fs/eventfd.h>
#include <fs/poll.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <mem/alloc.h>

struct eventfd {
    mutex_t lock;
    uint64_t count;
    bool semaphore;
    struct poll_event read_event;
    struct poll_event write_event;
};

static errno_t eventfd_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct eventfd* efd = file->priv;
    if (iovec_iter_len(buffer) < sizeof(uint64_t))
        return EINVAL;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &efd->read_event);

    errno_t status = 0;
    uint64_t value = 0;
    mutex_lock(&efd->lock);
    while (true) {
        if (efd->count > 0) {
            value = efd->semaphore ? 1 : efd->count;
            efd->count -= value;
            break;
        }
        if (vfs_file_nonblocking(file)) {
            status = EAGAIN;
            break;
        }
        if (poll_table_sleep(&table, &efd->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&efd->lock);
    poll_table_fini(&table);
    if (status)
        return status;

    poll_event_notify(&efd->write_event);
    size_t copied;
    status = iovec_iter_copy_to(buffer, &value, sizeof(value), &copied);
    *out_read = copied;
    return status;
}

static errno_t eventfd_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct eventfd* efd = file->priv;
    uint64_t value;
    size_t copied;
    if (iovec_iter_len(buffer) < sizeof(value))
        return EINVAL;
    errno_t status = iovec_iter_copy_from(buffer, &value, sizeof(value), &copied);
    if (status)
        return status;
    if (value == UINT64_MAX)
        return EINVAL;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &efd->write_event);

    mutex_lock(&efd->lock);
    while (true) {
        if (UINT64_MAX - 1 - efd->count >= value) {
            efd->count += value;
            break;
        }
        if (vfs_file_nonblocking(file)) {
            status = EAGAIN;
            break;
        }
        if (poll_table_sleep(&table, &efd->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&efd->lock);
    poll_table_fini(&table);
    if (status)
        return status;

    poll_event_notify(&efd->read_event);
    *out_written = sizeof(value);
    return 0;
}

static enum poll_flags eventfd_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct eventfd* efd = file->priv;
    poll_wait(table, &efd->read_event);
    poll_wait(table, &efd->write_event);

    enum poll_flags revents = 0;
    mutex_lock(&efd->lock);
    if (efd->count)
        revents |= POLLIN | POLLRDNORM;
    if (efd->count < UINT64_MAX - 1)
        revents |= POLLOUT | POLLWRNORM;
    mutex_unlock(&efd->lock);
    return revents;
}

static void eventfd_release(struct vfs_file* file) {
    mem_free(file->priv, sizeof(struct eventfd));
}

static const struct vfs_file_ops eventfd_ops = {
    .read = eventfd_read,
    .write = eventfd_write,
    .poll = eventfd_poll,
    .release = eventfd_release,
};

errno_t eventfd_create(
    uint64_t initval,
    bool semaphore,
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out
) {
    struct eventfd* efd = mem_alloc(sizeof(*efd), 0);
    if (!efd)
        return ENOMEM;
    mutex_init(&efd->lock);
    efd->count = initval;
    efd->semaphore = semaphore;
    poll_event_init(&efd->read_event);
    poll_event_init(&efd->write_event);

    struct vfs_node* node;
    errno_t status = vfs_create_anon_inode(&eventfd_ops, VNODE_ANONYMOUS, 0600, identity, &node);
    if (status) {
        mem_free(efd, sizeof(*efd));
        return status;
    }

    struct vfs_file* file;
    status = vfs_file_new(node, nullptr, O_RDWR | (flags & O_NONBLOCK), &file);
    vfs_node_unref(node);
    if (status) {
        mem_free(efd, sizeof(*efd));
        return status;
    }
    vfs_stream_open(file);

    file->priv = efd;
    *out = file;
    return 0;
}
