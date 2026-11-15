// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <zinnia/time.h>

#define SECS_TO_NS(s) (s * UINTMAX_C(1'000'000'000))

typedef struct __timespec timespec_t;

static inline timespec_t time_from_sec(uint64_t sec) {
    return (timespec_t){.tv_sec = sec, .tv_nsec = 0};
}

static inline timespec_t time_from_nano(uint64_t nsec) {
    return (timespec_t){.tv_sec = nsec / 1'000'000'000, .tv_nsec = nsec % 1'000'000'000};
}

static inline timespec_t time_from_milli(uint64_t msec) {
    return (timespec_t){.tv_sec = msec / 1'000, .tv_nsec = (msec % 1'000) * 1'000'000};
}
