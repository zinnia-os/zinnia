use crate::{
    arch::x86_64::{asm, consts},
    {
        boot::BootInfo,
        clock::{self, ClockSource},
    },
};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

static TSC_FREQUENCY: AtomicU64 = AtomicU64::new(0);
static TSC_BASE_CYCLES: AtomicU64 = AtomicU64::new(0);

fn cycles_to_ns(cycles: u64, frequency: u64) -> usize {
    let seconds = cycles / frequency;
    let remaining_cycles = cycles % frequency;

    let fractional_ns =
        (remaining_cycles as u128 * NANOS_PER_SECOND as u128 / frequency as u128) as u64;
    let ns = seconds
        .saturating_mul(NANOS_PER_SECOND)
        .saturating_add(fractional_ns);

    return ns.min(usize::MAX as u64) as usize;
}

struct TscClock;
impl ClockSource for TscClock {
    fn name(&self) -> &'static str {
        "tsc"
    }

    fn reset(&mut self) {
        // The TSC can't be set manually, so we record whatever value it had when `reset` was called and subtract that.
        TSC_BASE_CYCLES.store(asm::rdtsc(), Ordering::Relaxed);
    }

    fn get_priority(&self) -> u8 {
        // Prefer the TSC over other timers.
        return 255;
    }

    fn get_elapsed_ns(&self) -> usize {
        let cycles = asm::rdtsc() - TSC_BASE_CYCLES.load(Ordering::Relaxed);
        return cycles_to_ns(cycles, TSC_FREQUENCY.load(Ordering::Relaxed));
    }
}

#[initgraph::task(
    name = "arch.x86_64.tsc",
    depends = [super::hpet::HPET_STAGE],
    entails = [crate::clock::CLOCK_STAGE],
)]
fn TSC_STAGE() {
    // We need an invariant TSC.
    if asm::cpuid(1, 0).edx & consts::CPUID_1D_TSC == 0
        || asm::cpuid(0x8000_0007, 0).edx & (1 << 8) == 0
    {
        log!("No invariant TSC detected");
        return;
    }

    // Check if we have the TSC info leaf.
    let cpuid = match asm::cpuid(0, 0).eax >= 0x15 {
        true => Some(asm::cpuid(0x15, 0)),
        false => None,
    };

    // First, always try using another known good clock to calibrate.
    let freq = if clock::has_clock() {
        log!("Calibrating using existing clock");

        // Wait for 10ms.
        let t1 = asm::rdtsc();
        clock::block_ns(10_000_000).unwrap();
        let t2 = asm::rdtsc();

        // We want the frequency in Hz.
        // TODO: This might be imprecise.
        (t2 - t1) * 100
    } else if let Some(c) = cpuid {
        // If we have no timer (yet), the only way we can calibrate the TSC is if CPUID gives us the frequency.
        // On a normal system, this should usually never be called and is a last resort
        // since at this point we have at least the HPET timer.
        if c.ecx != 0 && c.ebx != 0 && c.eax != 0 {
            log!("Calibrating TSC using CPUID 0x15");
            c.ecx as u64 * c.ebx as u64 / c.eax as u64
        } else {
            log!("Unable to calibrate TSC using CPUID frequency information");
            return;
        }
    }
    // We tried.
    else {
        log!("Unable to calibrate TSC");
        return;
    };

    log!("Timer frequency is {} MHz ({} Hz)", freq / 1_000_000, freq);
    TSC_FREQUENCY.store(freq, Ordering::Relaxed);

    if BootInfo::get().command_line.get_bool("tsc").unwrap_or(true) {
        clock::switch(Box::new(TscClock)).unwrap();
    }
}
