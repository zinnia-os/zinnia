#include <kernel/clock.h>
#include <kernel/cmdline.h>
#include <kernel/print.h>
#include <string.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>

static bool use_tsc = true;

void tsc_init() {
    if (!(asm_cpuid(1, 0).edx & CPUID_1D_TSC) || !(asm_cpuid(0x8000'0007, 0).edx / (1 << 8))) {
        kprintf("No invariant TSC detected!\n");
        return;
    }

    // Check for the TSC info leaf.
    bool has_tsc_leaf = false;
    struct cpuid tsc_leaf = {0};
    if (asm_cpuid(0x8000'0000, 0).eax >= 0x15) {
        tsc_leaf = asm_cpuid(0x15, 0);
    }

    uint64_t freq = 0;
    if (clock_available()) {

    } else if (has_tsc_leaf) {
        // If we have no timer (yet), the only way we can calibrate the TSC is if CPUID gives us the frequency.
        // On a normal system, this should usually never be called and is a last resort
        // since at this point we have at least the HPET timer.
    } else {
        kprintf("Unable to calibrate the TSC without a clock or static frequency info!\n");
        return;
    }

    kprintf("Timer frequency is %lu MHz\n", freq / 1'000'000);
}

static void tsc_option(const char* opt) {
    if (!strcmp(opt, "off"))
        use_tsc = false;
}
CMDLINE_OPTION("tsc", tsc_option);
