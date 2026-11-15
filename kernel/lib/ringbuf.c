// SPDX-License-Identifier: BSD-3-Clause

#include <lib/ringbuf.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <string.h>

errno_t ringbuf_init(struct ringbuf* rb, size_t capacity) {
    rb->data = mem_alloc(capacity, ALLOC_NOZERO);
    if (!rb->data)
        return ENOMEM;
    rb->capacity = capacity;
    rb->head = 0;
    rb->len = 0;
    return 0;
}

void ringbuf_fini(struct ringbuf* rb) {
    mem_free(rb->data, rb->capacity);
    rb->data = nullptr;
    rb->capacity = 0;
    rb->len = 0;
}

static size_t tail(const struct ringbuf* rb) {
    return (rb->head + rb->len) % rb->capacity;
}

static size_t write_chunk(const struct ringbuf* rb, size_t len) {
    return MIN(len, MIN(ringbuf_space(rb), rb->capacity - tail(rb)));
}

static size_t read_chunk(const struct ringbuf* rb, size_t len) {
    return MIN(len, MIN(rb->len, rb->capacity - rb->head));
}

static void advance_read(struct ringbuf* rb, size_t len) {
    rb->head = (rb->head + len) % rb->capacity;
    rb->len -= len;
    if (rb->len == 0)
        rb->head = 0;
}

size_t ringbuf_write(struct ringbuf* rb, const void* src, size_t len) {
    size_t done = 0;
    size_t chunk;
    while ((chunk = write_chunk(rb, len - done))) {
        memcpy(rb->data + tail(rb), (const uint8_t*)src + done, chunk);
        rb->len += chunk;
        done += chunk;
    }
    return done;
}

size_t ringbuf_read(struct ringbuf* rb, void* dst, size_t len) {
    size_t done = 0;
    size_t chunk;
    while ((chunk = read_chunk(rb, len - done))) {
        memcpy((uint8_t*)dst + done, rb->data + rb->head, chunk);
        advance_read(rb, chunk);
        done += chunk;
    }
    return done;
}

errno_t ringbuf_write_iter(struct ringbuf* rb, iovec_iter_t* iter, size_t len, size_t* out_written) {
    size_t done = 0;
    errno_t status = 0;
    size_t chunk;
    while (!status && (chunk = write_chunk(rb, len - done))) {
        size_t copied;
        status = iovec_iter_copy_from(iter, rb->data + tail(rb), chunk, &copied);
        rb->len += copied;
        done += copied;
        if (copied < chunk)
            break;
    }
    *out_written = done;
    return done ? 0 : status;
}

errno_t ringbuf_read_iter(struct ringbuf* rb, iovec_iter_t* iter, size_t len, size_t* out_read) {
    size_t done = 0;
    errno_t status = 0;
    size_t chunk;
    while (!status && (chunk = read_chunk(rb, len - done))) {
        size_t copied;
        status = iovec_iter_copy_to(iter, rb->data + rb->head, chunk, &copied);
        advance_read(rb, copied);
        done += copied;
        if (copied < chunk)
            break;
    }
    *out_read = done;
    return done ? 0 : status;
}
