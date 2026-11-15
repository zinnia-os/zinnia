// SPDX-License-Identifier: BSD-3-Clause

#include <lib/compiler.h>
#include <lib/utils.h>
#include <stddef.h>
#include <stdint.h>

[[__hot]]
size_t strlen(const char* str) {
    if (!str)
        return 0;

    size_t result = 0;
    while (*str++) {
        result++;
    }
    return result;
}

[[__hot]]
size_t strnlen(const char* str, size_t len) {
    if (!str || len == 0)
        return 0;

    size_t result = 0;
    while (result < len && *str++) {
        result++;
    }
    return result;
}

[[__hot]]
int memcmp(const void* s1, const void* s2, size_t size) {
    const uint8_t* s1ptr = s1;
    const uint8_t* s2ptr = s2;

    for (size_t i = 0; i < size; i++) {
        if (s1ptr[i] != s2ptr[i])
            return (int)s1ptr[i] - (int)s2ptr[i];
    }
    return 0;
}

typedef size_t word_t [[__may_alias]]
;

[[__hot]]
void* memcpy(void* restrict dest, const void* restrict src, size_t n) {
    uint8_t* d = dest;
    const uint8_t* s = src;

    // Word copies are only possible if both pointers can be aligned at the same time.
    if (((uintptr_t)d - (uintptr_t)s) % sizeof(word_t) == 0) {
        while (n && (uintptr_t)d % sizeof(word_t) != 0) {
            *d++ = *s++;
            n--;
        }

        word_t* word_dest = (word_t*)d;
        const word_t* word_src = (const word_t*)s;
        for (; n >= sizeof(word_t); n -= sizeof(word_t))
            *word_dest++ = *word_src++;

        d = (uint8_t*)word_dest;
        s = (const uint8_t*)word_src;
    }

    while (n--)
        *d++ = *s++;

    return dest;
}

[[__hot]]
void* memmove(void* dstptr, const void* srcptr, size_t size) {
    uint8_t* dst = (uint8_t*)dstptr;
    const uint8_t* src = (const uint8_t*)srcptr;

    if ((uintptr_t)dst < (uintptr_t)src) {
        for (size_t i = 0; i < size; i++)
            dst[i] = src[i];
    } else {
        for (size_t i = size; i != 0; i--)
            dst[i - 1] = src[i - 1];
    }
    return dstptr;
}

[[__hot]]
void* memset(void* dest, int value, size_t n) {
    uint8_t* d = dest;
    const uint8_t byte = (uint8_t)value;

    while (n && (uintptr_t)d % sizeof(word_t) != 0) {
        *d++ = byte;
        n--;
    }

    const word_t word_value = (SIZE_MAX / 0xFF) * byte;
    word_t* word_dest = (word_t*)d;
    for (; n >= sizeof(word_t); n -= sizeof(word_t))
        *word_dest++ = word_value;

    d = (uint8_t*)word_dest;
    while (n--)
        *d++ = byte;

    return dest;
}

char* strncpy(char* restrict dst, const char* restrict src, size_t len) {
    size_t src_len = strnlen(src, len);
    memcpy(dst, src, src_len);
    memset(dst + src_len, 0, len - src_len);
    return dst;
}

int strncmp(const char* str1, const char* str2, size_t len) {
    while (len && *str1 && (*str1 == *str2)) {
        ++str1;
        ++str2;
        --len;
    }
    if (len == 0) {
        return 0;
    } else {
        return (*(unsigned char*)str1 - *(unsigned char*)str2);
    }
}

int strcmp(const char* str1, const char* str2) {
    while (*str1 && (*str1 == *str2)) {
        ++str1;
        ++str2;
    }
    return (*(unsigned char*)str1 - *(unsigned char*)str2);
}

char* strchr(const char* s, int c) {
    char ch = (char)c;
    while (true) {
        char cur = *s;
        if (cur == ch)
            return (char*)s;
        if (cur == 0)
            return nullptr;
        s++;
    }
}
