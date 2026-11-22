// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <stdckdint.h>
#include <stdint.h>
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

static inline errno_t time_to_nano(const timespec_t* ts, uint64_t* out) {
    if (ts->tv_sec < 0 || ts->tv_nsec < 0 || (uint64_t)ts->tv_nsec >= SECS_TO_NS(1))
        return EINVAL;
    if (ckd_mul(out, (uint64_t)ts->tv_sec, SECS_TO_NS(1)) || ckd_add(out, *out, (uint64_t)ts->tv_nsec))
        *out = UINT64_MAX;
    return 0;
}
