/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_RESOURCE_H
#define __ZINNIA_RESOURCE_H

#include <zinnia/time.h>
#include <zinnia/types.h>

#define __PRIO_PROCESS 0
#define __PRIO_PGRP    1
#define __PRIO_USER    2

#define __RLIMIT_CPU     0
#define __RLIMIT_FSIZE   1
#define __RLIMIT_DATA    2
#define __RLIMIT_STACK   3
#define __RLIMIT_CORE    4
#define __RLIMIT_RSS     5
#define __RLIMIT_NPROC   6
#define __RLIMIT_NOFILE  7
#define __RLIMIT_MEMLOCK 8
#define __RLIMIT_AS      9
#define __RLIMIT_NLIMITS 16

#define __RUSAGE_SELF     0
#define __RUSAGE_CHILDREN (-1)
#define __RUSAGE_THREAD   1

#define __RLIM_INFINITY ((__rlim_t) - 1)

struct __rlimit {
    __rlim_t rlim_cur;
    __rlim_t rlim_max;
};

struct __rusage {
    struct __timeval ru_utime;
    struct __timeval ru_stime;
    long ru_maxrss;
    long ru_ixrss;
    long ru_idrss;
    long ru_isrss;
    long ru_minflt;
    long ru_majflt;
    long ru_nswap;
    long ru_inblock;
    long ru_oublock;
    long ru_msgsnd;
    long ru_msgrcv;
    long ru_nsignals;
    long ru_nvcsw;
    long ru_nivcsw;
};

#endif
