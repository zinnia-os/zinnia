// SPDX-License-Identifier: BSD-3-Clause

#include <dev/char/chardev.h>
#include <dev/char/devctl.h>
#include <fs/poll.h>
#include <lib/assert.h>
#include <lib/atomic.h>
#include <lib/mutex.h>
#include <lib/tailq.h>
#include <mem/alloc.h>
#include <string.h>

constexpr size_t DEVCTL_QUEUE_CAPACITY = 256;
constexpr size_t DEVCTL_BACKLOG_CAPACITY = 256;

struct devctl_line {
    tailq_link(struct devctl_line) link;
    size_t len;
    char data[];
};

struct devctl_queue {
    tailq_head(struct devctl_line) lines;
    size_t count;
};

struct devctl_reader {
    tailq_link(struct devctl_reader) link;
    struct devctl_queue queue;
};

static mutex_t devctl_lock;
static tailq_head(struct devctl_reader) devctl_readers;
static struct devctl_queue devctl_backlog;
static bool devctl_had_reader;
static struct poll_event devctl_event;
static atomic_bool devctl_ready;
static struct chardev devctl_dev;

static void queue_init(struct devctl_queue* queue) {
    tailq_init(&queue->lines);
    queue->count = 0;
}

static struct devctl_line* queue_pop(struct devctl_queue* queue) {
    struct devctl_line* line = tailq_first(&queue->lines);
    if (line) {
        tailq_remove(&queue->lines, line, link);
        queue->count--;
    }
    return line;
}

static void line_free(struct devctl_line* line) {
    mem_free(line, sizeof(*line) + line->len);
}

static void queue_push(struct devctl_queue* queue, struct devctl_line* line, size_t capacity) {
    if (queue->count >= capacity)
        line_free(queue_pop(queue));
    tailq_insert_tail(&queue->lines, line, link);
    queue->count++;
}

static void queue_clear(struct devctl_queue* queue) {
    struct devctl_line* line;
    while ((line = queue_pop(queue)))
        line_free(line);
}

static struct devctl_line* line_new(const char* data, size_t len) {
    struct devctl_line* line = mem_alloc(sizeof(*line) + len, 0);
    if (!line)
        return nullptr;
    line->len = len;
    memcpy(line->data, data, len);
    return line;
}

static void devctl_notify(const char* type, const char* name) {
    if (!atomic_load(&devctl_ready, ATOMIC_ACQUIRE))
        return;

    while (*name == '/')
        name++;

    const char prefix[] = "!system=DEVFS subsystem=CDEV type=";
    const char middle[] = " cdev=";
    const size_t prefix_len = sizeof(prefix) - 1;
    const size_t type_len = strlen(type);
    const size_t middle_len = sizeof(middle) - 1;
    const size_t name_len = strlen(name);
    const size_t len = prefix_len + type_len + middle_len + name_len + 1;

    char* buf = mem_alloc(len, 0);
    if (!buf)
        return;
    char* pos = buf;
    memcpy(pos, prefix, prefix_len);
    pos += prefix_len;
    memcpy(pos, type, type_len);
    pos += type_len;
    memcpy(pos, middle, middle_len);
    pos += middle_len;
    memcpy(pos, name, name_len);
    pos += name_len;
    *pos = '\n';

    mutex_lock(&devctl_lock);
    if (tailq_empty(&devctl_readers)) {
        if (!devctl_had_reader) {
            struct devctl_line* line = line_new(buf, len);
            if (line)
                queue_push(&devctl_backlog, line, DEVCTL_BACKLOG_CAPACITY);
        }
    } else {
        for (struct devctl_reader* reader = tailq_first(&devctl_readers); reader; reader = tailq_next(reader, link)) {
            struct devctl_line* line = line_new(buf, len);
            if (line)
                queue_push(&reader->queue, line, DEVCTL_QUEUE_CAPACITY);
        }
    }
    mutex_unlock(&devctl_lock);
    mem_free(buf, len);

    poll_event_notify(&devctl_event);
}

void devctl_notify_create(const char* name) {
    devctl_notify("CREATE", name);
}

void devctl_notify_destroy(const char* name) {
    devctl_notify("DESTROY", name);
}

static bool file_nonblocking(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK;
}

static errno_t drain_lines(struct devctl_queue* queue, iovec_iter_t* buffer, size_t* out_total) {
    size_t total = 0;
    errno_t status = 0;
    struct devctl_line* line;
    while ((line = tailq_first(&queue->lines))) {
        if (line->len > iovec_iter_len(buffer)) {
            if (total == 0)
                status = EINVAL;
            break;
        }
        queue_pop(queue);
        size_t copied;
        status = iovec_iter_copy_to(buffer, line->data, line->len, &copied);
        line_free(line);
        total += copied;
        if (status)
            break;
    }
    *out_total = total;
    return status;
}

static errno_t devctl_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct devctl_reader* reader = file->priv;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &devctl_event);

    errno_t status = 0;
    size_t read = 0;
    mutex_lock(&devctl_lock);
    while (true) {
        if (reader->queue.count) {
            status = drain_lines(&reader->queue, buffer, &read);
            break;
        }
        if (file_nonblocking(file)) {
            status = EAGAIN;
            break;
        }
        if (poll_table_sleep(&table, &devctl_lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&devctl_lock);
    poll_table_fini(&table);

    *out_read = read;
    return status;
}

static errno_t devctl_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    return EBADF;
}

static enum poll_flags devctl_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct devctl_reader* reader = file->priv;

    if (events & (POLLIN | POLLRDNORM))
        poll_wait(table, &devctl_event);

    enum poll_flags revents = 0;
    mutex_lock(&devctl_lock);
    if (reader->queue.count)
        revents |= POLLIN | POLLRDNORM;
    mutex_unlock(&devctl_lock);
    return revents;
}

static void devctl_release(struct vfs_file* file) {
    struct devctl_reader* reader = file->priv;

    mutex_lock(&devctl_lock);
    tailq_remove(&devctl_readers, reader, link);
    mutex_unlock(&devctl_lock);

    queue_clear(&reader->queue);
    mem_free(reader, sizeof(*reader));
}

static const struct vfs_file_ops devctl_file_ops = {
    .read = devctl_read,
    .write = devctl_write,
    .poll = devctl_poll,
    .release = devctl_release,
};

static errno_t devctl_open(struct chardev* dev, struct vfs_file* file) {
    struct devctl_reader* reader = mem_alloc(sizeof(*reader), 0);
    if (!reader)
        return ENOMEM;
    queue_init(&reader->queue);

    mutex_lock(&devctl_lock);
    if (!devctl_had_reader) {
        devctl_had_reader = true;
        struct devctl_line* line;
        while ((line = queue_pop(&devctl_backlog)))
            queue_push(&reader->queue, line, DEVCTL_QUEUE_CAPACITY);
    }
    tailq_insert_tail(&devctl_readers, reader, link);
    mutex_unlock(&devctl_lock);

    file->priv = reader;
    vfs_stream_open(file);
    return 0;
}

static const struct chardev_ops devctl_dev_ops = {.open = devctl_open};

static void devctl_stage_init(struct boot_info* info) {
    mutex_init(&devctl_lock);
    tailq_init(&devctl_readers);
    queue_init(&devctl_backlog);
    poll_event_init(&devctl_event);
    atomic_store(&devctl_ready, true, ATOMIC_RELEASE);

    chardev_init(&devctl_dev, &devctl_dev_ops, &devctl_file_ops, 5, 3);
    errno_t status = devtmpfs_add_chardev("devctl", &devctl_dev, 0600);
    assert(status == 0, "Unable to create /dev/devctl (%i)", status);
}

INIT_TASK(devctl_stage, "dev.char.devctl", devctl_stage_init);
INIT_DEPENDS(devctl_stage, devtmpfs_stage);
