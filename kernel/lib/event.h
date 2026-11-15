// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

struct event {
    _Atomic(uint32_t) state;
};

void event_init(struct event* event);

// Sets the event and wakes all waiters.
void event_set(struct event* event);

// Clears the event, so that future waiters block again.
void event_reset(struct event* event);

bool event_is_set(struct event* event);

// Blocks until the event is set.
void event_wait(struct event* event);
