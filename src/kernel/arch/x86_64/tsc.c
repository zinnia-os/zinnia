#include <kernel/clock.h>
#include <kernel/cmdline.h>
#include <kernel/print.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>

static bool use_tsc = true;

void tsc_init() {
    if (!use_tsc)
        return;

    if (!(asm_cpuid(1, 0).edx & CPUID_1D_TSC) || !(asm_cpuid(0x8000'0007, 0).edx & (1 << 8))) {
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
        // Calibrate using existing clock.
        uint64_t t1 = asm_rdtsc();
        clock_spin_ns(10'000'000);
        uint64_t t2 = asm_rdtsc();
        freq = (t2 - t1) * 100;
    } else if (has_tsc_leaf) {
        // If we have no timer (yet), the only way we can calibrate the TSC is if CPUID gives us the frequency.
        // On a normal system, this should usually never be called and is a last resort
        // since at this point we have at least the HPET timer.
        if (tsc_leaf.ecx != 0 && tsc_leaf.ebx != 0 && tsc_leaf.eax != 0) {
            kprintf("Calibrating TSC using CPUID 0x15");
            freq = tsc_leaf.ecx * tsc_leaf.ebx / tsc_leaf.eax;
        } else {
            kprintf("Unable to calibrate TSC using CPUID frequency information");
            return;
        }
    } else {
        kprintf("Unable to calibrate the TSC without a clock or static frequency info!\n");
        return;
    }

    kprintf("Timer frequency is %lu MHz\n", freq / 1'000'000);
}

static void tsc_option(bool opt) {
    use_tsc = opt;
}
CMDLINE_OPTION("tsc", tsc_option);
