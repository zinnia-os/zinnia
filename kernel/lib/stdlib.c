// SPDX-License-Identifier: BSD-3-Clause

#include <lib/stdlib.h>
#include <stdint.h>
#include <string.h>

static void reverse(char* s) {
    int i, j;
    char c;

    for (i = 0, j = strlen(s) - 1; i < j; i++, j--) {
        c = s[i];
        s[i] = s[j];
        s[j] = c;
    }
}

static int char_to_digit(char c) {
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'z')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'Z')
        return c - 'A' + 10;
    return -1;
}

unsigned long atolu(const char* str, int base) {
    unsigned long result = 0;

    if (base == 16 && str[0] == '0' && (str[1] == 'x' || str[1] == 'X')) {
        str += 2;
    }

    while (*str) {
        int digit = char_to_digit(*str++);
        if (digit < 0 || digit >= base)
            break;

        result = result * base + digit;
    }

    return result;
}

long atol(const char* str, int base) {
    int neg = 0;

    if (*str == '-') {
        neg = 1;
        str++;
    }

    unsigned long val = atolu(str, base);
    return neg ? -(long)val : (long)val;
}

char* lutoa(uint64_t value, char* str, int base) {
    if (base < 2 || base > 36)
        return nullptr;

    size_t i = 0;

    do {
        uint64_t digit = value % base;
        str[i++] = (digit < 10) ? '0' + digit : 'a' + (digit - 10);
        value /= base;
    } while (value);

    str[i] = '\0';
    reverse(str);
    return str;
}

char* ltoa(int64_t value, char* str, int base) {
    if (base < 2 || base > 36)
        return nullptr;

    uint64_t magnitude;
    int negative = 0;

    if (value < 0) {
        negative = 1;
        magnitude = (uint64_t)(-(value + 1)) + 1;
        // avoids INT64_MIN overflow
    } else {
        magnitude = (uint64_t)value;
    }

    size_t i = 0;

    do {
        uint64_t digit = magnitude % base;
        str[i++] = (digit < 10) ? '0' + digit : 'a' + (digit - 10);
        magnitude /= base;
    } while (magnitude);

    if (negative)
        str[i++] = '-';

    str[i] = '\0';
    reverse(str);
    return str;
}
