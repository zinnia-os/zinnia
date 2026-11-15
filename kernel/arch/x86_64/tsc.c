// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "x86_64/tsc: " fmt
#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <lib/clock.h>
#include <lib/cmdline.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <stdint.h>

static bool use_tsc = true;
static void tsc_option(bool opt) {
    use_tsc = opt;
}
CMDLINE_OPTION("x86_64.tsc", "Enable timestamp counter", tsc_option);

constexpr uint64_t nanos_per_sec = 1'000'000'000;
static uint64_t tsc_freq = 0;
static uint64_t tsc_base_cycles = 0;

static uint64_t cycles_to_ns(uint64_t cycles, uint64_t freq) {
    uint64_t seconds = cycles / freq;
    uint64_t remaining_cycles = cycles % freq;
    uint64_t fractional_ns = (uint64_t)((__uint128_t)remaining_cycles * (__uint128_t)nanos_per_sec / (__uint128_t)freq);
    uint64_t ns = seconds * nanos_per_sec + fractional_ns;

    return MIN(ns, UINT64_MAX);
}

static void tsc_reset(struct clock* clock) {
    tsc_base_cycles = asm_rdtsc();
}

static uint64_t tsc_get_elapsed_ns(struct clock* clock) {
    uint64_t cycles = asm_rdtsc() - tsc_base_cycles;
    return cycles_to_ns(cycles, tsc_freq);
}

static struct clock tsc_clock = {
    .name = "tsc",
    .priority = 255,
    .reset = tsc_reset,
    .get_elapsed_ns = tsc_get_elapsed_ns,
};

void tsc_init() {
    if (!use_tsc) {
        log_warn("TSC disabled on the command line");
        return;
    }

    if (!(asm_cpuid(1, 0).edx & CPUID_1D_TSC) || !(asm_cpuid(0x8000'0007, 0).edx & (1 << 8))) {
        log_warn("No invariant TSC detected!\n");
        return;
    }

    // Check for the TSC info leaf.
    bool has_tsc_leaf = false;
    struct cpuid tsc_leaf = {0};
    if (asm_cpuid(0, 0).eax >= 0x15) {
        tsc_leaf = asm_cpuid(0x15, 0);
        has_tsc_leaf = true;
    }

    if (clock_available()) {
        // Calibrate using existing clock.
        uint64_t t1 = asm_rdtsc();
        clock_spin_ns(10'000'000);
        uint64_t t2 = asm_rdtsc();
        tsc_freq = (t2 - t1) * 100;
    } else if (has_tsc_leaf) {
        // If we have no timer (yet), the only way we can calibrate the TSC is if CPUID gives us the frequency.
        // On a normal system, this should usually never be called and is a last resort
        // since at this point we have at least the HPET timer.
        if (tsc_leaf.ecx != 0 && tsc_leaf.ebx != 0 && tsc_leaf.eax != 0) {
            log("Calibrating TSC using CPUID 0x15");
            tsc_freq = (uint64_t)tsc_leaf.ecx * tsc_leaf.ebx / tsc_leaf.eax;
        } else {
            log_err("Unable to calibrate TSC using CPUID frequency information");
            return;
        }
    } else {
        log_err("Unable to calibrate the TSC without a clock or static frequency info!\n");
        return;
    }

    log("Timer frequency is %lu MHz\n", tsc_freq / 1'000'000);
    clock_switch(&tsc_clock);
}
