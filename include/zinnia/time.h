/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_TIME_H
#define __ZINNIA_TIME_H

#include <zinnia/types.h>

typedef __zn_i64 __time_t;
typedef __zn_i64 __suseconds_t;

struct __timespec {
    __time_t tv_sec;
    long tv_nsec;
};

struct __timeval {
    __time_t tv_sec;
    __suseconds_t tv_usec;
};

struct __itimerval {
    struct __timeval it_interval;
    struct __timeval it_value;
};

struct __itimerspec {
    struct __timespec it_interval;
    struct __timespec it_value;
};

#define __ITIMER_REAL    0
#define __ITIMER_VIRTUAL 1
#define __ITIMER_PROF    2

#define __CLOCK_REALTIME           0
#define __CLOCK_MONOTONIC          1
#define __CLOCK_PROCESS_CPUTIME_ID 2
#define __CLOCK_THREAD_CPUTIME_ID  3
#define __CLOCK_MONOTONIC_RAW      4
#define __CLOCK_REALTIME_COARSE    5
#define __CLOCK_MONOTONIC_COARSE   6
#define __CLOCK_BOOTTIME           7

#define __TFD_CLOEXEC       __O_CLOEXEC
#define __TFD_NONBLOCK      __O_NONBLOCK
#define __TFD_TIMER_ABSTIME 1

#define __TIMER_ABSTIME 1

#endif
