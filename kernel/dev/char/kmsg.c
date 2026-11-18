// SPDX-License-Identifier: BSD-3-Clause

#include <dev/char/chardev.h>
#include <fs/poll.h>
#include <lib/assert.h>
#include <lib/mutex.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <string.h>
#include <zinnia/ioctls.h>
#include <zinnia/termios.h>

constexpr size_t KMSG_HEADER_MAX = 80;
constexpr size_t KMSG_LINE_MAX = KMSG_HEADER_MAX + LOG_MESSAGE_MAX + 1;

// A single reader that has opened the kmsg device.
struct kmsg_reader {
    mutex_t lock;
    uint64_t seq;
    struct log_entry entry;
    char line[KMSG_LINE_MAX];
};

static const uint8_t syslog_priority[] = {
    [LOG_DEBUG] = 7,
    [LOG_STATUS] = 5,
    [LOG_INFO] = 6,
    [LOG_WARN] = 4,
    [LOG_ERR] = 3,
    [LOG_PANIC] = 0,
};

static bool file_nonblocking(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK;
}

// Converts from a log entry to plain text which can be parsed.
static size_t format_entry(const struct log_entry* entry, char* out) {
    size_t len = ksnprintf(
        out,
        KMSG_HEADER_MAX,
        "%u,%lu,%lu,-,caller=T%u;",
        (unsigned int)syslog_priority[entry->level],
        entry->seq,
        entry->time_ns / 1'000,
        (unsigned int)entry->tid
    );
    memcpy(out + len, entry->msg, entry->len);
    len += entry->len;
    out[len++] = '\n';
    return len;
}

static errno_t kmsg_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct kmsg_reader* reader = file->priv;
    *out_read = 0;

    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, log_poll_event());

    errno_t status;
    mutex_lock(&reader->lock);
    while (true) {
        uint64_t seq = reader->seq;
        status = log_read(&seq, &reader->entry);
        if (status == EPIPE) {
            reader->seq = seq;
            break;
        }

        if (status == 0) {
            // If we successfully read a line from the log, convert it to plain text.
            const size_t len = format_entry(&reader->entry, reader->line);

            // Buffer can't fit the log anymore, stop here.
            if (len > iovec_iter_len(buffer)) {
                status = EINVAL;
                break;
            }

            size_t copied;
            status = iovec_iter_copy_to(buffer, reader->line, len, &copied);

            if (!status) {
                reader->seq = seq + 1;
                *out_read = copied;
            }
            break;
        }

        // If we can't block, return.
        if (file_nonblocking(file))
            break;

        // Wait for more lines to appear.
        if (poll_table_sleep(&table, &reader->lock, nullptr) == EINTR) {
            status = ERESTART;
            break;
        }
    }
    mutex_unlock(&reader->lock);
    poll_table_fini(&table);
    return status;
}

static errno_t kmsg_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    return EBADF;
}

static enum poll_flags kmsg_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct kmsg_reader* reader = file->priv;

    if (events & (POLLIN | POLLRDNORM))
        poll_wait(table, log_poll_event());

    mutex_lock(&reader->lock);
    const bool ready = reader->seq < log_next_seq();
    mutex_unlock(&reader->lock);

    return ready ? POLLIN | POLLRDNORM : 0;
}

static void kmsg_release(struct vfs_file* file) {
    mem_free(file->priv, sizeof(struct kmsg_reader));
}

static const struct vfs_file_ops kmsg_file_ops = {
    .read = kmsg_read,
    .write = kmsg_write,
    .poll = kmsg_poll,
    .release = kmsg_release,
};

static errno_t kmsg_open(struct chardev* dev, struct vfs_file* file) {
    struct kmsg_reader* reader = mem_alloc(sizeof(struct kmsg_reader), 0);
    if (!reader)
        return ENOMEM;

    mutex_init(&reader->lock);
    file->priv = reader;
    vfs_stream_open(file);

    return 0;
}

static const struct chardev_ops kmsg_dev_ops = {
    .open = kmsg_open,
};

static struct chardev kmsg_dev;

static void kmsg_stage_init(struct boot_info* info) {
    chardev_init(&kmsg_dev, &kmsg_dev_ops, &kmsg_file_ops, 1, 11);
    errno_t status = devtmpfs_add_chardev("kmsg", &kmsg_dev, 0666);
    assert(status == 0, "Unable to create /dev/kmsg (%i)", status);
}

INIT_TASK(kmsg_stage, "dev.char.kmsg", kmsg_stage_init);
INIT_DEPENDS(kmsg_stage, devtmpfs_stage);
