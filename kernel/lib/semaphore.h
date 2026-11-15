// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

typedef struct {
    _Atomic(uint32_t) count;
} sem_t;

void sem_init(sem_t* sem, uint32_t count);
bool sem_trywait(sem_t* sem);
bool sem_timedwait(sem_t* sem, const uint64_t* deadline_ns);
void sem_wait(sem_t* sem);
void sem_post(sem_t* sem);
