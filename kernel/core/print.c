#include <kernel/console.h>
#include <kernel/panic.h>
#include <kernel/print.h>
#include <stdarg.h>
#include <stddef.h>
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

static int pow(int x, unsigned int y) {
    if (y == 0)
        return 1;
    else if ((y % 2) == 0)
        return pow(x, y / 2) * pow(x, y / 2);
    else
        return x * pow(x, y / 2) * pow(x, y / 2);
}

long atol(const char* str, int base) {
    size_t len = strlen(str);
    int64_t result = 0;
    size_t i = 0;
    if (str[0] == '-')
        i++;
    for (; i < len; i++)
        result += (str[i] - '0') * pow(base, len - i - 1);
    if (str[0] == '-')
        result *= -1;
    return result;
};

unsigned long atolu(const char* str, int base) {
    size_t len = strlen(str);
    uint64_t result = 0;
    size_t i = 0;
    for (; i < len; i++)
        result += (str[i] - '0') * pow(base, len - i - 1);
    return result;
};

char* ltoa(int64_t value, char* str, int base) {
    size_t i;
    int64_t sign;
    sign = value;
    if (base == 10 && sign < 0)
        value = -value;
    i = 0;
    do {
        char c = value % base;
        if (c > 9)
            c += 7;
        str[i++] = c + '0';
    } while ((value /= base) > 0);
    if (base == 10 && sign < 0)
        str[i++] = '-';
    str[i] = '\0';
    reverse(str);
    return str;
};

char* lutoa(uint64_t value, char* str, int base) {
    size_t i = 0;
    do {
        char c = value % base;
        if (c > 9)
            c += 7;
        str[i++] = c + '0';
    } while ((value /= base) > 0);
    str[i] = '\0';
    reverse(str);
    return str;
};

typedef void (*log_fn_t)(void* ctx, const char* buf, size_t len);

static void kvprintf(log_fn_t callback, void* ctx, const char* restrict fmt, va_list args) {
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
        bool has_width = false;
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
            has_width = true;
            char number[10 + 1] = {0};
            size_t idx = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                number[idx++] = *fmt;
                fmt++;
            }
            width = atolu(number, 10);

            goto check_fmt;
        }
        // Precision
        case '.': {
            has_precision = true;
            fmt++;
            if (*fmt == '*') {
                precision = (char)va_arg(args, int32_t);
            } else {
                char number[10 + 1] = {0};
                size_t idx = 0;
                while (*fmt >= '0' && *fmt <= '9') {
                    number[idx++] = *fmt;
                    fmt++;
                }
                precision = atolu(number, 10);
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

            callback(ctx, &c, sizeof(c));

            written++;
            fmt++;
            break;
        }
        // String of characters
        case 's': {
            const char* str = va_arg(args, const char*);
            if (!str)
                str = "(null)";
            size_t len = 0;
            if (has_width)
                len = strnlen(str, width);
            else
                len = strlen(str);

            callback(ctx, str, len);

            written += len;
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
print_num:
            size_t len = strlen(number);
            if (has_precision) {
                for (size_t i = 0; i < precision - len; i++)
                    callback(ctx, " ", 1);
            }
            if (has_width) {
                char c = pad_zero ? '0' : ' ';

                // We don't have to pad anything.
                if (len < width) {
                    for (size_t i = 0; i < width - len; i++)
                        callback(ctx, &c, 1);
                }
            }
            if (write_prefix) {
                callback(ctx, "0x", 2);
            }
            if (force_sign) {
                char c = '+';
                if (number[0] != '-')
                    callback(ctx, &c, 1);
            }
            if (blank_sign) {
                char c = ' ';
                if (number[0] != '-')
                    callback(ctx, &c, 1);
            }
            callback(ctx, number, len);

            written += len;
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
            written += len;
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
            fmt = format_begun_at;
            size_t len = strlen(fmt);

            callback(ctx, fmt, len);

            written += len;
            fmt += len;
            break;
        }
        }
    }
}

static void printf_console(void*, const char* msg, size_t len) {
    console_write(msg, len);
}

void kprintf(const char* message, ...) {
    va_list args;
    va_start(args, message);

    kvprintf(printf_console, nullptr, message, args);

    va_end(args);
}
