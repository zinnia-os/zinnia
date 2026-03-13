#pragma once

#include <stdint.h>

typedef intptr_t time_t;
typedef intptr_t suseconds_t;

struct timespec {
    time_t tv_sec;
    intptr_t tv_nsec;
};

struct timeval {
    time_t tv_sec;
    suseconds_t tv_usec;
};
