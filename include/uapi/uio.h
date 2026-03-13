#pragma once

#include <stddef.h>

struct iovec {
    void* base;
    size_t len;
};
