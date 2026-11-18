// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/iovec.h>
#include <stddef.h>
#include <stdint.h>

struct ringbuf {
    uint8_t* data;
    size_t capacity;
    size_t head;
    size_t len;
};

errno_t ringbuf_init(struct ringbuf* rb, size_t capacity);
void ringbuf_fini(struct ringbuf* rb);

static inline size_t ringbuf_len(const struct ringbuf* rb) {
    return rb->len;
}

static inline size_t ringbuf_space(const struct ringbuf* rb) {
    return rb->capacity - rb->len;
}

static inline void ringbuf_clear(struct ringbuf* rb) {
    rb->head = 0;
    rb->len = 0;
}

size_t ringbuf_write(struct ringbuf* rb, const void* src, size_t len);
size_t ringbuf_read(struct ringbuf* rb, void* dst, size_t len);
errno_t ringbuf_write_iter(struct ringbuf* rb, iovec_iter_t* iter, size_t len, size_t* out_written);
errno_t ringbuf_read_iter(struct ringbuf* rb, iovec_iter_t* iter, size_t len, size_t* out_read);
size_t ringbuf_peek(const struct ringbuf* rb, size_t offset, void* dst, size_t len);
errno_t ringbuf_peek_iter(const struct ringbuf* rb, size_t offset, iovec_iter_t* iter, size_t len, size_t* out_read);
void ringbuf_discard(struct ringbuf* rb, size_t len);
