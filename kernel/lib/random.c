// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "random: " fmt

#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/print.h>
#include <lib/random.h>
#include <lib/spin.h>
#include <string.h>

static _Atomic(uint64_t) state = 0x9E37'79B9'7F4A'7C15;

static bool dumb_random(struct random_source*, uint64_t* out) {
    uint64_t z = atomic_fetch_add(&state, 0x9E37'79B9'7F4A'7C15, ATOMIC_RELAXED);
    z ^= clock_get_elapsed_ns() * 0x2545'F491'4F6C'DD1D;
    z = (z ^ (z >> 30)) * 0xBF58'476D'1CE4'E5B9;
    z = (z ^ (z >> 27)) * 0x94D0'49BB'1331'11EB;
    *out = z ^ (z >> 31);
    return true;
}

static struct random_source fallback_source = {
    .name = "fallback",
    .quality = 0,
    .get_u64 = dumb_random,
};

static _Atomic(struct random_source*) active_source = &fallback_source;
static struct spinlock lock = {};

bool random_register(struct random_source* source) {
    spin_lock(&lock);

    struct random_source* active = atomic_load(&active_source, ATOMIC_RELAXED);
    if (source->quality <= active->quality) {
        spin_unlock(&lock);
        return false;
    }
    atomic_store(&active_source, source, ATOMIC_RELEASE);

    spin_unlock(&lock);

    log("Switching to random source \"%s\"\n", source->name);

    return true;
}

void random_fill(void* buf, size_t len) {
    struct random_source* source = atomic_load(&active_source, ATOMIC_ACQUIRE);
    uint8_t* out = buf;
    while (len) {
        uint64_t value;
        source->get_u64(source, &value);

        const size_t chunk = len < sizeof(value) ? len : sizeof(value);
        memcpy(out, &value, chunk);
        out += chunk;
        len -= chunk;
    }
}

INIT_TASK(random_stage, "lib.random");
