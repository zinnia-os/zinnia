// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/errno.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>

[[__format(printf, 1, 2)]]
void kprintf(const char* message, ...);

void kvprintf(const char* message, va_list args);

[[__format(printf, 3, 4)]]
size_t ksnprintf(char* buf, size_t size, const char* format, ...);

size_t kvsnprintf(char* buf, size_t size, const char* format, va_list args);

typedef void (*log_fn_t)(void* ctx, const char* buf, size_t len);
void print_fmt(log_fn_t callback, void* ctx, const char* restrict fmt, va_list args);

enum log_level {
    LOG_DEBUG,
    LOG_STATUS,
    LOG_INFO,
    LOG_WARN,
    LOG_ERR,
    LOG_PANIC,
};

[[__format(printf, 2, 3)]]
void log_write(enum log_level level, const char* message, ...);

void log_vwrite(enum log_level level, const char* message, va_list args);

// Like log_vwrite(), but prepends a `prefix` to the message.
void log_vwrite_prefixed(enum log_level level, const char* prefix, const char* message, va_list args);

// Passes every record currently in the log buffer to `callback`.
void log_replay(log_fn_t callback, void* ctx);

constexpr size_t LOG_MESSAGE_MAX = 255;

struct log_entry {
    uint64_t seq;
    uint64_t time_ns;
    uint32_t tid;
    enum log_level level;
    size_t len;
    char msg[LOG_MESSAGE_MAX];
};

struct poll_event;

errno_t log_read(uint64_t* seq, struct log_entry* out);
uint64_t log_next_seq();
struct poll_event* log_poll_event();

#ifndef LOG_FMT
#define LOG_FMT(fmt) fmt
#endif

#define log(message, ...)        log_write(LOG_INFO, LOG_FMT(message), ##__VA_ARGS__)
#define log_debug(message, ...)  log_write(LOG_DEBUG, LOG_FMT(message), ##__VA_ARGS__)
#define log_status(message, ...) log_write(LOG_STATUS, LOG_FMT(message), ##__VA_ARGS__)
#define log_warn(message, ...)   log_write(LOG_WARN, LOG_FMT(message), ##__VA_ARGS__)
#define log_err(message, ...)    log_write(LOG_ERR, LOG_FMT(message), ##__VA_ARGS__)
#define log_panic(message, ...)  log_write(LOG_PANIC, LOG_FMT(message), ##__VA_ARGS__)
