// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/event.h>
#include <lib/park.h>
#include <lib/percpu.h>
#include <lib/sched.h>

constexpr uint32_t EVENT_SET = (1u << 0);
constexpr uint32_t EVENT_PARKED = (1u << 1); // At least one task might be parked on the event.

void event_init(struct event* event) {
    atomic_store(&event->state, 0, ATOMIC_RELAXED);
}

void event_set(struct event* event) {
    uint32_t old = atomic_fetch_or(&event->state, EVENT_SET, ATOMIC_RELEASE);
    if (!(old & EVENT_PARKED))
        return;

    atomic_fetch_and(&event->state, ~EVENT_PARKED, ATOMIC_RELAXED);
    unpark_all(event, 0);
}

void event_reset(struct event* event) {
    atomic_fetch_and(&event->state, ~EVENT_SET, ATOMIC_RELAXED);
}

bool event_is_set(struct event* event) {
    return atomic_load(&event->state, ATOMIC_ACQUIRE) & EVENT_SET;
}

void event_wait(struct event* event) {
    uint32_t state = atomic_load(&event->state, ATOMIC_ACQUIRE);
    while (!(state & EVENT_SET)) {
        if (!(state & EVENT_PARKED)) {
            if (!atomic_compare_exchange_weak(
                    &event->state,
                    &state,
                    state | EVENT_PARKED,
                    ATOMIC_ACQUIRE,
                    ATOMIC_ACQUIRE
                ))
                continue;
        }

        // Only sleeps if the event is not set, but parked at this point.
        park_wait(&event->state, EVENT_PARKED);
        state = atomic_load(&event->state, ATOMIC_ACQUIRE);
    }
}
