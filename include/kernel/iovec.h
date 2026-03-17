#pragma once

#include <uapi/uio.h>

typedef struct iovec iovec_t;

// Returns the total length of all vectors.
size_t iovec_len(iovec_t* iovec, size_t num);

typedef struct {
    iovec_t* base;
    size_t count;

    iovec_t* current;
    size_t current_offset;

    size_t total_offset;
    size_t total_size;
} iovec_iter_t;

// Initializes a new iterator.
void iovec_iter_init(iovec_iter_t* iter, iovec_t* iovec, size_t count);
