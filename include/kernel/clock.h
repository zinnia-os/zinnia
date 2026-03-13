#pragma once

#include <stdint.h>

struct clock {
    const char* name;
    uint8_t priority;

    void (*reset)(struct clock* c);
    uint64_t (*get_elapsed_ns)(struct clock* c);
};

// Attempts to set a new clock. Returns true if the clock was changed.
bool clock_switch(struct clock* clock);

// Returns true, if a clock is available.
bool clock_available();

// Gets the current timestamp in nanoseconds since the last reset.
uint64_t clock_get_elapsed_ns();

// Spin for `ns` nanoseconds.
void clock_spin_ns(uint64_t ns);
