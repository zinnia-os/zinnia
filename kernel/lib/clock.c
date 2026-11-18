// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "clock: " fmt

#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/compiler.h>
#include <lib/cpu.h>
#include <lib/print.h>
#include <stdint.h>

static _Atomic(struct clock*) active_clock;
static _Atomic(int64_t) realtime_offset_ns;

static uint64_t clock_read_elapsed(struct clock* clock) {
    if (__unlikely(!clock))
        return 0;

    return clock->base + clock->get_elapsed_ns(clock);
}

bool clock_available() {
    return atomic_load(&active_clock, ATOMIC_ACQUIRE) != nullptr;
}

uint64_t clock_get_elapsed_ns() {
    return clock_read_elapsed(atomic_load(&active_clock, ATOMIC_ACQUIRE));
}

uint64_t clock_get_realtime_ns() {
    int64_t ns = (int64_t)clock_get_elapsed_ns() + atomic_load(&realtime_offset_ns, ATOMIC_RELAXED);
    return ns < 0 ? 0 : ns;
}

void clock_set_realtime_ns(uint64_t ns) {
    atomic_store(&realtime_offset_ns, (int64_t)ns - (int64_t)clock_get_elapsed_ns(), ATOMIC_RELAXED);
}

bool clock_switch(struct clock* clock) {
    struct clock* old = atomic_load(&active_clock, ATOMIC_ACQUIRE);

    do {
        if (old && clock->priority <= old->priority)
            return false;

        uint64_t elapsed = clock_read_elapsed(old);
        clock->reset(clock);
        clock->base = elapsed;
    } while (!atomic_compare_exchange(&active_clock, &old, clock, ATOMIC_RELEASE, ATOMIC_ACQUIRE));

    log("Switching to clock source \"%s\"\n", clock->name);

    return true;
}

void clock_spin_ns(uint64_t ns) {
    if (!clock_available()) {
        log_err("Unable to sleep for %lu ns, no clock available!\n", ns);
        return;
    }

    uint64_t target = clock_get_elapsed_ns() + ns;
    while (clock_get_elapsed_ns() < target) {
        cpu_pause();
    }
}
