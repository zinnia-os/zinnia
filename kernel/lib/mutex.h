// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

typedef _Atomic(uintptr_t) mutex_t;

void mutex_init(mutex_t* mutex);

void mutex_lock(mutex_t* mutex);

bool mutex_trylock(mutex_t* mutex);

void mutex_unlock(mutex_t* mutex);
