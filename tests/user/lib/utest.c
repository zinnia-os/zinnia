// SPDX-License-Identifier: BSD-3-Clause

#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <tests/user/lib/syscall.h>
#include <tests/user/lib/utest.h>

constexpr size_t LINE_MAX = 240;

extern const struct utest_case __start_utest_cases[];
extern const struct utest_case __stop_utest_cases[];

size_t strlen(const char* str) {
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

void* memset(void* dest, int value, size_t n) {
    uint8_t* d = dest;
    for (size_t i = 0; i < n; i++)
        d[i] = (uint8_t)value;
    return dest;
}

bool utest_streq(const char* a, const char* b) {
    if (!a || !b)
        return a == b;
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return *a == *b;
}

struct line {
    char buf[LINE_MAX];
    size_t len;
};

static void put(struct line* line, char c) {
    if (line->len < sizeof(line->buf) - 1)
        line->buf[line->len++] = c;
}

static void put_str(struct line* line, const char* str) {
    for (; *str; str++)
        put(line, *str);
}

static void put_num(struct line* line, unsigned long long value, unsigned base, bool negative, bool prefix) {
    char digits[24];
    size_t n = 0;
    do {
        digits[n++] = "0123456789abcdef"[value % base];
        value /= base;
    } while (value);
    if (negative)
        put(line, '-');
    if (prefix)
        put_str(line, "0x");
    while (n)
        put(line, digits[--n]);
}

static void format(struct line* line, const char* fmt, va_list args) {
    for (; *fmt; fmt++) {
        if (*fmt != '%') {
            put(line, *fmt);
            continue;
        }
        fmt++;
        bool alt = false;
        if (*fmt == '#') {
            alt = true;
            fmt++;
        }
        int longs = 0;
        bool size = false;
        for (;; fmt++) {
            if (*fmt == 'l')
                longs++;
            else if (*fmt == 'z')
                size = true;
            else
                break;
        }

        switch (*fmt) {
        case 'd':
        case 'i': {
            long long v = size ? (long long)va_arg(args, ptrdiff_t)
                : longs        ? va_arg(args, long long)
                               : va_arg(args, int);
            put_num(line, v < 0 ? -(unsigned long long)v : (unsigned long long)v, 10, v < 0, false);
            break;
        }
        case 'u':
        case 'x': {
            unsigned long long v = size ? va_arg(args, size_t)
                : longs                 ? va_arg(args, unsigned long long)
                                        : va_arg(args, unsigned int);
            put_num(line, v, *fmt == 'x' ? 16 : 10, false, alt && *fmt == 'x');
            break;
        }
        case 'p':
            put_num(line, (uintptr_t)va_arg(args, void*), 16, false, true);
            break;
        case 's': {
            const char* s = va_arg(args, const char*);
            put_str(line, s ? s : "(null)");
            break;
        }
        case 'c':
            put(line, (char)va_arg(args, int));
            break;
        case '%':
            put(line, '%');
            break;
        default:
            put(line, '?');
            break;
        }
    }
}

static void emit(struct line* line) {
    line->buf[line->len] = 0;
    sys_syslog(SYSLOG_INFO, line->buf, line->len);
}

void utest_log(const char* fmt, ...) {
    struct line line = {};
    va_list args;
    va_start(args, fmt);
    format(&line, fmt, args);
    va_end(args);
    emit(&line);
}

void utest_fail(struct utest* test, const char* file, int line_nr, const char* fmt, ...) {
    struct line line = {};
    put_str(&line, "# ");
    put_str(&line, test->suite);
    put(&line, '.');
    put_str(&line, test->name);
    put_str(&line, ": ");
    put_str(&line, file);
    put(&line, ':');
    put_num(&line, (unsigned)line_nr, 10, false, false);
    put_str(&line, ": ");
    va_list args;
    va_start(args, fmt);
    format(&line, fmt, args);
    va_end(args);
    emit(&line);
    test->failed = true;
}

void utest_run_all() {
    size_t index = 0;
    for (const struct utest_case* c = __start_utest_cases; c < __stop_utest_cases; c++) {
        struct utest test = {.suite = c->suite, .name = c->name};
        c->func(&test);

        index++;
        if (test.skipped)
            utest_log("ok %zu %s.%s # SKIP %s", index, c->suite, c->name, test.skip_reason ? test.skip_reason : "");
        else if (c->xfail_reason)
            utest_log(
                "%s %zu %s.%s # TODO %s",
                test.failed ? "not ok" : "ok",
                index,
                c->suite,
                c->name,
                c->xfail_reason
            );
        else
            utest_log("%s %zu %s.%s", test.failed ? "not ok" : "ok", index, c->suite, c->name);
    }

    utest_log("1..%zu", index);
}

int main() {
    utest_run_all();
    for (;;)
        ;
}
