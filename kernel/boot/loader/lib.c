// SPDX-License-Identifier: BSD-3-Clause

#include <stddef.h>
#include <stdint.h>
#include <string.h>

size_t strlen(const char* str) {
    if (!str)
        return 0;

    size_t len = 0;
    while (str[len])
        len++;
    return len;
}

int memcmp(const void* s1, const void* s2, size_t size) {
    const uint8_t* a = s1;
    const uint8_t* b = s2;
    for (size_t i = 0; i < size; i++) {
        if (a[i] != b[i])
            return a[i] < b[i] ? -1 : 1;
    }
    return 0;
}

void* memcpy(void* restrict dest, const void* restrict src, size_t n) {
    uint8_t* d = dest;
    const uint8_t* s = src;
    for (size_t i = 0; i < n; i++)
        d[i] = s[i];
    return dest;
}

void* memmove(void* dstptr, const void* srcptr, size_t size) {
    uint8_t* d = dstptr;
    const uint8_t* s = srcptr;
    if (d < s) {
        for (size_t i = 0; i < size; i++)
            d[i] = s[i];
    } else {
        for (size_t i = size; i > 0; i--)
            d[i - 1] = s[i - 1];
    }
    return dstptr;
}

void* memset(void* dest, int value, size_t n) {
    uint8_t* d = dest;
    for (size_t i = 0; i < n; i++)
        d[i] = (uint8_t)value;
    return dest;
}
