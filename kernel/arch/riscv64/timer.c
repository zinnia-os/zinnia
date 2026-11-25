// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "riscv64/timer: " fmt

#include <arch/riscv64/sbi.h>
#include <dev/dt/fdt.h>
#include <lib/clock.h>
#include <lib/initgraph.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/timer.h>
#include <lib/utils.h>
#include <config.h>
#include <stdint.h>

#if CONFIG_ACPI
#include <dev/acpi/acpi.h>
#include <uacpi/acpi.h>
#include <uacpi/status.h>
#include <uacpi/tables.h>
#endif

constexpr uint64_t NANOS_PER_SEC = 1'000'000'000;

static uint64_t timebase_freq = 0;
static uint64_t base_ticks = 0;

static inline uint64_t read_time() {
    uint64_t time;
    asm volatile("rdtime %0" : "=r"(time));
    return time;
}

static uint64_t ticks_to_ns(uint64_t ticks) {
    return ticks / timebase_freq * NANOS_PER_SEC + ticks % timebase_freq * NANOS_PER_SEC / timebase_freq;
}

static uint64_t ns_to_ticks(uint64_t ns) {
    return (uint64_t)((__uint128_t)ns * timebase_freq / NANOS_PER_SEC);
}

static void time_reset(struct clock* clock) {
    base_ticks = read_time();
}

static uint64_t time_get_elapsed_ns(struct clock* clock) {
    return ticks_to_ns(read_time() - base_ticks);
}

static struct clock time_clock = {
    .name = "rdtime",
    .priority = 255,
    .reset = time_reset,
    .get_elapsed_ns = time_get_elapsed_ns,
};

void arch_timer_program(uint64_t deadline_ns) {
    if (!timebase_freq)
        return;
    const uint64_t now = clock_get_elapsed_ns();
    const uint64_t delta = deadline_ns > now ? deadline_ns - now : 0;
    sbi_set_timer(read_time() + MAX(ns_to_ticks(delta), 1));
}

[[__init]]
static void time_fdt_init(struct boot_info*) {
    uint32_t freq;
    if (!fdt_property_u32(fdt_find(fdt_get(), "/cpus"), "timebase-frequency", &freq) || !freq)
        return;

    timebase_freq = freq;
    log("Timebase frequency is %lu Hz\n", timebase_freq);
    clock_switch(&time_clock);
}
INIT_TASK(time_fdt_stage, "arch.riscv64.time.fdt", time_fdt_init);
INIT_DEPENDS(time_fdt_stage, fdt_stage);
INIT_ENTAILS(time_fdt_stage, percpu_stage);

#if CONFIG_ACPI

// Find the timer frequency using the ACPI RHCT.
[[__init]]
static void time_acpi_init(struct boot_info*) {
    uacpi_table table = {0};
    if (uacpi_table_find_by_signature(ACPI_RHCT_SIGNATURE, &table) != UACPI_STATUS_OK)
        return;
    const uint64_t freq = ((struct acpi_rhct*)table.ptr)->timebase_frequency;
    uacpi_table_unref(&table);
    if (!freq)
        return;

    timebase_freq = freq;
    log("Timebase frequency is %lu Hz\n", timebase_freq);
    clock_switch(&time_clock);
}
INIT_TASK(time_acpi_stage, "arch.riscv64.time.acpi", time_acpi_init);
INIT_DEPENDS(time_acpi_stage, acpi_stage);
INIT_ENTAILS(time_acpi_stage, percpu_stage);

#endif
