#include <kernel/iovec.h>

size_t iovec_len(iovec_t* iovec, size_t num) {
    size_t size = 0;

    for (size_t i = 0; i < num; i++)
        size += iovec[i].len;

    return size;
}

iovec_iter_t iovec_iter_new(iovec_t* iovec, size_t count) {
    iovec_iter_t iter = {
        .base = iovec,
        .count = count,

        .current = iovec,
        .current_offset = 0,

        .total_offset = 0,
        .total_size = iovec_len(iovec, count),
    };

    return iter;
}
