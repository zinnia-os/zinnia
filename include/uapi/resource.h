#pragma once

#include <uapi/time.h>
#include <uapi/types.h>

#define RUSAGE_SELF     0
#define RUSAGE_CHILDREN -1

#define RLIMIT_CPU        0
#define RLIMIT_FSIZE      1
#define RLIMIT_DATA       2
#define RLIMIT_STACK      3
#define RLIMIT_CORE       4
#define RLIMIT_RSS        5
#define RLIMIT_NPROC      6
#define RLIMIT_NOFILE     7
#define RLIMIT_MEMLOCK    8
#define RLIMIT_AS         9
#define RLIMIT_LOCKS      10
#define RLIMIT_SIGPENDING 11
#define RLIMIT_MSGQUEUE   12
#define RLIMIT_NICE       13
#define RLIMIT_RTPRIO     14
#define RLIMIT_RTTIME     15
#define RLIMIT_NLIMITS    16

#define PRIO_PROCESS 1
#define PRIO_PGRP    2
#define PRIO_USER    3

#define PRIO_MIN (-20)
#define PRIO_MAX 20

#define RLIM_INFINITY  ((rlim_t) - 1)
#define RLIM_SAVED_MAX ((rlim_t) - 1)
#define RLIM_SAVED_CUR ((rlim_t) - 1)

#define RLIM_NLIMITS RLIMIT_NLIMITS

struct rusage {
    struct timeval ru_utime;
    struct timeval ru_stime;
    intptr_t ru_maxrss;
    intptr_t ru_ixrss;
    intptr_t ru_idrss;
    intptr_t ru_isrss;
    intptr_t ru_minflt;
    intptr_t ru_majflt;
    intptr_t ru_nswap;
    intptr_t ru_inblock;
    intptr_t ru_oublock;
    intptr_t ru_msgsnd;
    intptr_t ru_msgrcv;
    intptr_t ru_nsignals;
    intptr_t ru_nvcsw;
    intptr_t ru_nivcsw;
};

struct rlimit {
    rlim_t rlim_cur;
    rlim_t rlim_max;
};
