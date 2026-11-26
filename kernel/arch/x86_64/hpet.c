// SPDX-License-Identifier: BSD-3-Clause

#include <dev/acpi/acpi.h>
#include <lib/assert.h>
#include <lib/clock.h>
#include <lib/cmdline.h>
#include <lib/initgraph.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/mmio.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <uacpi/acpi.h>
#include <uacpi/status.h>
#include <uacpi/tables.h>
#include <uacpi/uacpi.h>

static bool use_hpet = true;
static void hpet_option(bool use) {
    use_hpet = use;
}
CMDLINE_OPTION("x86_64.hpet", "Enable High Precision Event Timer clock", hpet_option);

struct hpet {
    struct clock clock;
    void* regs;
    uint32_t period;
};

constexpr mmio_reg64 HPET_REG_CAP = {.offset = 0x0};
constexpr mmio_field64 HPET_CAP_PERIOD = {HPET_REG_CAP, 32, 32};
constexpr mmio_reg64 HPET_REG_CFG = {.offset = 0x10};
constexpr mmio_field64 HPET_CFG_ENABLE = {HPET_REG_CFG, 0, 1};
constexpr mmio_reg64 HPET_REG_COUNTER = {.offset = 0xF0};

static void hpet_reset(struct clock* c) {
    struct hpet* hpet = containerof(c, struct hpet, clock);
    mmio_reg_write(hpet->regs, HPET_REG_COUNTER, 0);
}

static uint64_t hpet_get_elapsed_ns(struct clock* c) {
    struct hpet* hpet = containerof(c, struct hpet, clock);
    uint64_t counter = mmio_reg_read(hpet->regs, HPET_REG_COUNTER);
    return (uint64_t)((__uint128_t)counter * (__uint128_t)hpet->period / 1'000'000);
}

static struct hpet hpet_clock = {
    .clock = {
        .name = "hpet",
        .priority = 128,
        .reset = hpet_reset,
        .get_elapsed_ns = hpet_get_elapsed_ns,
    },
};

static void hpet_init(struct boot_info*) {
    if (!use_hpet)
        return;

    uacpi_table table = {0};
    if (uacpi_table_find_by_signature("HPET", &table) != UACPI_STATUS_OK) {
        log("x86_64/hpet: No HPET table found\n");
        return;
    }

    struct acpi_hpet* hpet = table.ptr;

    uintptr_t vaddr = vm_alloc(0x1000);
    pmap_map(&kernel_vma.pmap, vaddr, hpet->address.address, PTE_READ | PTE_WRITE, CACHE_MMIO);
    hpet_clock.regs = (void*)vaddr;

    uacpi_table_unref(&table);
    log("x86_64/hpet: HPET registers mapped at %p\n", hpet_clock.regs);

    uint64_t cfg = mmio_reg_read(hpet_clock.regs, HPET_REG_CFG);
    cfg |= mmio_field(HPET_CFG_ENABLE, 1);
    mmio_reg_write(hpet_clock.regs, HPET_REG_CFG, cfg);

    uint64_t cap = mmio_reg_read(hpet_clock.regs, HPET_REG_CAP);
    hpet_clock.period = mmio_field_get(HPET_CAP_PERIOD, cap);

    assert(clock_switch(&hpet_clock.clock), "Unable to switch to HPET!");
}
INIT_TASK(hpet_stage, "arch.x86_64.hpet", hpet_init);
INIT_DEPENDS(hpet_stage, acpi_stage);
INIT_ENTAILS(hpet_stage, percpu_stage);
