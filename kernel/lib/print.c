// SPDX-License-Identifier: BSD-3-Clause

#include <lib/clock.h>
#include <lib/console.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/spin.h>
#include <lib/stdlib.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

static void print_pad(log_fn_t callback, void* ctx, char c, size_t count) {
    for (size_t i = 0; i < count; i++)
        callback(ctx, &c, 1);
}

void print_fmt(log_fn_t callback, void* ctx, const char* restrict fmt, va_list args) {
    // Amount of bytes written.
    int32_t written = 0;

    while (*fmt != '\0') {
        size_t maxrem = INT32_MAX - written;

        if (fmt[0] != '%' || fmt[1] == '%') {
            if (fmt[0] == '%')
                fmt++;
            size_t amount = 1;
            while (fmt[amount] && fmt[amount] != '%')
                amount++;
            if (maxrem < amount) {
                return;
            }
            callback(ctx, fmt, amount);
            fmt += amount;
            written += amount;
            continue;
        }

        const char* format_begun_at = fmt++;
        bool force_sign = false;
        bool blank_sign = false;
        bool write_prefix = false;
        bool pad_zero = false;
        bool left_align = false;
        size_t width = 0;
        bool has_precision = false;
        size_t precision = 0;
        size_t size = sizeof(uint32_t);
        char number[32];
check_fmt:
        memset(number, 0, sizeof(number));

        switch (*fmt) {
        // Flags
        case '+': {
            force_sign = true;
            fmt++;
            goto check_fmt;
        }
        case ' ': {
            blank_sign = true;
            fmt++;
            goto check_fmt;
        }
        case '#': {
            write_prefix = true;
            fmt++;
            goto check_fmt;
        }
        case '0': {
            pad_zero = true;
            fmt++;
            goto check_fmt;
        }
        case '-': {
            left_align = true;
            fmt++;
            goto check_fmt;
        }
        // Width
        case '1':
        case '2':
        case '3':
        case '4':
        case '5':
        case '6':
        case '7':
        case '8':
        case '9': {
            width = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                width = width * 10 + (*fmt - '0');
                fmt++;
            }

            goto check_fmt;
        }
        // Precision
        case '.': {
            has_precision = true;
            fmt++;
            precision = 0;
            if (*fmt == '*') {
                const int32_t arg = va_arg(args, int32_t);
                if (arg < 0)
                    has_precision = false;
                else
                    precision = arg;
                fmt++;
            } else {
                while (*fmt >= '0' && *fmt <= '9') {
                    precision = precision * 10 + (*fmt - '0');
                    fmt++;
                }
            }
            goto check_fmt;
        }
        // Length
        case 'h': {
            if (fmt[1] == 'h') {
                size = sizeof(uint8_t);
                fmt++;
            } else
                size = sizeof(uint16_t);
            fmt++;
            goto check_fmt;
        }
        case 'l': {
            // There is no difference between `long int` and `long long int`.
            if (fmt[1] == 'l')
                fmt++;
            fmt++;
            size = sizeof(uint64_t);

            goto check_fmt;
        }
        case 'z': {
            size = sizeof(size_t);
            fmt++;
            goto check_fmt;
        }
        // Character
        case 'c': {
            char c = (char)va_arg(args, int32_t);
            const size_t pad = width > 1 ? width - 1 : 0;

            if (!left_align)
                print_pad(callback, ctx, ' ', pad);
            callback(ctx, &c, sizeof(c));
            if (left_align)
                print_pad(callback, ctx, ' ', pad);

            written += 1 + pad;
            fmt++;
            break;
        }
        // String of characters
        case 's': {
            const char* str = va_arg(args, const char*);
            if (!str)
                str = "(null)";
            size_t len = 0;
            if (has_precision)
                len = strnlen(str, precision);
            else
                len = strlen(str);
            const size_t pad = width > len ? width - len : 0;

            if (!left_align)
                print_pad(callback, ctx, ' ', pad);
            callback(ctx, str, len);
            if (left_align)
                print_pad(callback, ctx, ' ', pad);

            written += len + pad;
            fmt++;
            break;
        }
        // Signed decimal integer
        case 'i':
        case 'd': {
            switch (size) {
            case sizeof(int8_t):
                ltoa((int8_t)va_arg(args, int32_t), number, 10);
                break;
            case sizeof(int16_t):
                ltoa((int16_t)va_arg(args, int32_t), number, 10);
                break;
            case sizeof(int32_t):
                ltoa((int32_t)va_arg(args, int32_t), number, 10);
                break;
            default:
                ltoa(va_arg(args, int64_t), number, 10);
                break;
            }
            goto print_num;
        }
        // Unsigned decimal integer
        case 'u': {
            switch (size) {
            case sizeof(uint8_t):
                lutoa((uint8_t)va_arg(args, uint32_t), number, 10);
                break;
            case sizeof(uint16_t):
                lutoa((uint16_t)va_arg(args, uint32_t), number, 10);
                break;
            case sizeof(uint32_t):
                lutoa(va_arg(args, uint32_t), number, 10);
                break;
            case sizeof(uint64_t):
                lutoa(va_arg(args, uint64_t), number, 10);
                break;
            }
            goto print_num;
        }
        // Unsigned hexadecimal integer
        case 'X':
        case 'x': {
            switch (size) {
            case sizeof(uint8_t):
                lutoa((uint8_t)va_arg(args, uint32_t), number, 16);
                break;
            case sizeof(uint16_t):
                lutoa((uint16_t)va_arg(args, uint32_t), number, 16);
                break;
            case sizeof(uint32_t):
                lutoa(va_arg(args, uint32_t), number, 16);
                break;
            case sizeof(uint64_t):
                lutoa(va_arg(args, uint64_t), number, 16);
                break;
            }
            if (*fmt == 'X') {
                for (char* c = number; *c; c++) {
                    if (*c >= 'a' && *c <= 'f')
                        *c -= 'a' - 'A';
                }
            }
print_num:
            const char* digits = number;
            char sign = 0;
            if (digits[0] == '-') {
                sign = '-';
                digits++;
            } else if (force_sign) {
                sign = '+';
            } else if (blank_sign) {
                sign = ' ';
            }

            const size_t len = strlen(digits);
            size_t zeros = (has_precision && precision > len) ? precision - len : 0;
            const size_t total = (sign ? 1 : 0) + (write_prefix ? 2 : 0) + zeros + len;
            size_t pad = width > total ? width - total : 0;
            if (pad_zero && !left_align && !has_precision) {
                zeros += pad;
                pad = 0;
            }

            if (!left_align)
                print_pad(callback, ctx, ' ', pad);
            if (sign)
                callback(ctx, &sign, 1);
            if (write_prefix)
                callback(ctx, *fmt == 'X' ? "0X" : "0x", 2);
            print_pad(callback, ctx, '0', zeros);
            callback(ctx, digits, len);
            if (left_align)
                print_pad(callback, ctx, ' ', pad);

            written += width > total ? width : total;
            fmt++;
            break;
        }
        // Pointer address
        case 'p': {
            const size_t num = va_arg(args, size_t);
            const size_t buf_size = sizeof(size_t) * 2 + 1;
            char str[buf_size];
            lutoa(num, number, 0x10);
            const size_t len = strlen(number); // Get the length of the final number.
            for (size_t i = 0; i < buf_size; i++)
                str[i] = '0';
            size_t offset = sizeof(str) - len - 1;

            callback(ctx, str, offset);
            callback(ctx, number, len);

            fmt++;
            written += offset + len;
            break;
        }
        // The number of characters written so far is stored in the pointed location.
        case 'n': {
            int32_t* const ptr = va_arg(args, int32_t*);
            *ptr = written;
            fmt++;
            break;
        }
        // No format, just normal text.
        default: {
            size_t len = fmt - format_begun_at;
            if (*fmt)
                len++;

            callback(ctx, format_begun_at, len);

            written += len;
            fmt = format_begun_at + len;
            break;
        }
        }
    }
}

static void printf_console(void*, const char* msg, size_t len) {
    console_write(msg, len);
}

static struct spinlock print_lock = {};

void kprintf(const char* message, ...) {
    spin_lock(&print_lock);

    va_list args;
    va_start(args, message);
    print_fmt(printf_console, nullptr, message, args);
    va_end(args);

    spin_unlock(&print_lock);
}

struct snprintf_ctx {
    char* buf;
    size_t size;
    size_t len;
};

static void snprintf_write(void* ctx, const char* str, size_t len) {
    struct snprintf_ctx* c = ctx;
    // Always keep room for the terminator.
    if (c->len < c->size) {
        size_t room = c->size - c->len - 1;
        memcpy(c->buf + c->len, str, len < room ? len : room);
    }
    c->len += len;
}

size_t kvsnprintf(char* buf, size_t size, const char* format, va_list args) {
    struct snprintf_ctx ctx = {.buf = buf, .size = size};
    print_fmt(snprintf_write, &ctx, format, args);
    if (size > 0)
        buf[ctx.len < size ? ctx.len : size - 1] = '\0';
    return ctx.len;
}

size_t ksnprintf(char* buf, size_t size, const char* format, ...) {
    va_list args;
    va_start(args, format);
    size_t len = kvsnprintf(buf, size, format, args);
    va_end(args);
    return len;
}

void kvprintf(const char* message, va_list args) {
    spin_lock(&print_lock);

    print_fmt(printf_console, nullptr, message, args);

    spin_unlock(&print_lock);
}

struct log_record {
    uint64_t time_ns;
    uint32_t tid;
    uint16_t len;
    uint8_t level;
};

#define LOG_BUFFER_SIZE (64 * 1024)
#define LOG_MESSAGE_MAX 255

static uint8_t log_buffer[LOG_BUFFER_SIZE];

// Protected by print_lock.
static size_t log_head = 0;
static size_t log_tail = 0;

static void log_ring_copy_in(const void* src, size_t len) {
    for (size_t i = 0; i < len; i++)
        log_buffer[(log_head + i) % LOG_BUFFER_SIZE] = ((const uint8_t*)src)[i];
    log_head += len;
}

static void log_ring_peek(size_t offset, void* dst, size_t len) {
    for (size_t i = 0; i < len; i++)
        ((uint8_t*)dst)[i] = log_buffer[(offset + i) % LOG_BUFFER_SIZE];
}

static void log_store(const struct log_record* rec, const char* msg) {
    const size_t total = sizeof(*rec) + rec->len;

    // Drop the oldest records until the new one fits.
    while (LOG_BUFFER_SIZE - (log_head - log_tail) < total) {
        struct log_record old;
        log_ring_peek(log_tail, &old, sizeof(old));
        log_tail += sizeof(old) + old.len;
    }

    log_ring_copy_in(rec, sizeof(*rec));
    log_ring_copy_in(msg, rec->len);
}

struct log_line {
    char buf[LOG_MESSAGE_MAX];
    size_t len;
};

static void log_line_append(void* ctx, const char* str, size_t len) {
    struct log_line* line = ctx;
    if (len > sizeof(line->buf) - line->len)
        len = sizeof(line->buf) - line->len;
    memcpy(line->buf + line->len, str, len);
    line->len += len;
}

// ANSI colors for each level.
static const char* const log_level_colors[] = {
    [LOG_DEBUG] = "\e[90m",
    [LOG_STATUS] = "\e[32m",
    [LOG_INFO] = "",
    [LOG_WARN] = "\e[33m",
    [LOG_ERR] = "\e[31m",
    [LOG_PANIC] = "\e[1;31m",
};

static void log_cb_printf(log_fn_t callback, void* ctx, const char* fmt, ...) {
    va_list args;
    va_start(args, fmt);
    print_fmt(callback, ctx, fmt, args);
    va_end(args);
}

// Formats a single record and passes it to `callback`.
static void log_emit(log_fn_t callback, void* ctx, const struct log_record* rec, const char* msg) {
    log_cb_printf(
        callback,
        ctx,
        "[%5lu.%06lu] %s",
        rec->time_ns / 1'000'000'000,
        rec->time_ns / 1'000 % 1'000'000,
        log_level_colors[rec->level]
    );
    callback(ctx, msg, rec->len);
    callback(ctx, "\e[0m\n", 5);
}

void log_replay(log_fn_t callback, void* ctx) {
    char msg[LOG_MESSAGE_MAX];
    for (size_t offset = log_tail; offset != log_head;) {
        struct log_record rec;
        log_ring_peek(offset, &rec, sizeof(rec));
        log_ring_peek(offset + sizeof(rec), msg, rec.len);
        log_emit(callback, ctx, &rec, msg);
        offset += sizeof(rec) + rec.len;
    }
}

void log_vwrite(enum log_level level, const char* message, va_list args) {
    log_vwrite_prefixed(level, "", message, args);
}

void log_vwrite_prefixed(enum log_level level, const char* prefix, const char* message, va_list args) {
    struct log_line line = {};
    log_line_append(&line, prefix, strlen(prefix));
    print_fmt(log_line_append, &line, message, args);
    if (line.len > 0 && line.buf[line.len - 1] == '\n')
        line.len--;

    auto cur = percpu_get()->sched.current;
    const struct log_record rec = {
        .time_ns = clock_get_elapsed_ns(),
        .tid = cur ? cur->id : 0,
        .len = line.len,
        .level = level,
    };

    spin_lock(&print_lock);
    log_store(&rec, line.buf);
    log_emit(printf_console, nullptr, &rec, line.buf);
    spin_unlock(&print_lock);
}

void log_write(enum log_level level, const char* message, ...) {
    va_list args;
    va_start(args, message);
    log_vwrite(level, message, args);
    va_end(args);
}
