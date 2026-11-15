// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/atomic.h>

struct rwlock {
    atomic_uintptr_t word;
};

void rwlock_init(struct rwlock* rw);

void rwlock_read_lock(struct rwlock* rw);
void rwlock_read_unlock(struct rwlock* rw);
bool rwlock_try_read_lock(struct rwlock* rw);

void rwlock_write_lock(struct rwlock* rw);
void rwlock_write_unlock(struct rwlock* rw);
bool rwlock_try_write_lock(struct rwlock* rw);

void rwlock_downgrade(struct rwlock* rw);
bool rwlock_try_upgrade(struct rwlock* rw);
