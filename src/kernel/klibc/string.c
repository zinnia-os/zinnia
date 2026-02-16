#include <common/compiler.h>
#include <kernel/common.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

[[__hot]]
size_t strlen(const char* str) {
    size_t result = 0;
    while (*str++) {
        result++;
    }
    return result;
}

[[__hot]]
size_t strnlen(const char* str, size_t len) {
    size_t result = 0;
    while (result < len && *str++) {
        result++;
    }
    return result;
}

[[__hot]]
int memcmp(const void* s1, const void* s2, size_t size) {
    int diff = 0;
    char* s1ptr = (char*)s1;
    char* s2ptr = (char*)s2;

    for (size_t i = 0; i < size; i++) {
        if (s1ptr[i] != s2ptr[i])
            diff++;
    }
    return diff;
}

[[__hot]]
void* memcpy(void* restrict dest, const void* restrict src, size_t n) {
    if (__unlikely(n == 0))
        return dest;

    size_t d = (size_t)dest;
    size_t s = (size_t)src;

    if (d % sizeof(size_t) != 0 || s % sizeof(size_t) != 0) {
        while (n && (d % sizeof(size_t) != 0) && (s % sizeof(size_t) != 0)) {
            *((uint8_t*)d) = *((uint8_t*)s);
            d++;
            s++;
            n--;
        }
    }

    size_t* qword_dest = (size_t*)d;
    const size_t* qword_src = (const size_t*)s;
    const size_t word_count = n / sizeof(size_t);

    for (size_t i = 0; i < word_count; i++) {
        qword_dest[i] = qword_src[i];
    }

    size_t remaining_bytes = n % sizeof(size_t);
    d = (size_t)(qword_dest + word_count);
    s = (size_t)(qword_src + word_count);

    while (remaining_bytes--) {
        *((uint8_t*)d) = *((uint8_t*)s);
        d++;
        s++;
    }

    return dest;
}

[[__hot]]
void* memmove(void* dstptr, const void* srcptr, size_t size) {
    uint8_t* dst = (uint8_t*)dstptr;
    const uint8_t* src = (const uint8_t*)srcptr;
    if (dst < src) {
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
    if (__unlikely(n == 0))
        return dest;

    size_t d = (size_t)dest;

    if (d % sizeof(size_t) != 0) {
        while (n && (d % sizeof(size_t) != 0)) {
            *((uint8_t*)d) = value;
            d++;
            n--;
        }
    }

    size_t* qword_dest = (size_t*)d;
    size_t qword_value = value;
    qword_value |= (qword_value << 8);
    qword_value |= (qword_value << 16);
    qword_value |= (qword_value << 32);

    const size_t word_count = n / sizeof(size_t);

    for (size_t i = 0; i < word_count; i++) {
        qword_dest[i] = qword_value;
    }

    size_t remaining_bytes = n % sizeof(size_t);
    d = (size_t)(qword_dest + word_count);

    while (remaining_bytes--) {
        *((uint8_t*)d) = value;
        d++;
    }

    return dest;
}

char* strncpy(char* restrict dst, const char* restrict src, size_t len) {
    size_t src_len = strnlen(src, len) + 1;
    return memcpy(dst, src, MIN(len, src_len));
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
