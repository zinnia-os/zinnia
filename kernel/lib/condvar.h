// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/mutex.h>
#include <stdint.h>

struct condvar {
    _Atomic(uint32_t) seq;
    _Atomic(uint32_t) waiters;
};

void condvar_init(struct condvar* cv);

void condvar_wait(struct condvar* cv, mutex_t* mutex);

void condvar_signal(struct condvar* cv);

void condvar_broadcast(struct condvar* cv);
