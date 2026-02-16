#pragma once

#include <stdint.h>

struct clock {
    uint64_t (*get_elapsed_ns)();
    void (*reset)();

    const char* name;
    uint8_t priority;
};

// Attempts to set a new clock. Returns true if the clock was changed.
bool clock_switch(struct clock* clock);

// Returns true, if a clock is available.
bool clock_available();

// Gets the current timestamp in nanoseconds since the last reset.
uint64_t clock_get_elapsed_ns();
