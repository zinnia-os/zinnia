#include <kernel/iovec.h>

size_t iovec_len(iovec_t* iovec, size_t num) {
    size_t size = 0;

    for (size_t i = 0; i < num; i++)
        size += iovec[i].len;

    return size;
}

void iovec_iter_init(iovec_iter_t* iter, iovec_t* iovec, size_t count) {
    iter->base = iovec;
    iter->count = count;

    iter->current = iovec;
    iter->current_offset = 0;

    iter->total_offset = 0;
    iter->total_size = iovec_len(iovec, count);
}
