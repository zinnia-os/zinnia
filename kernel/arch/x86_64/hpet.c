#include <kernel/assert.h>
#include <kernel/clock.h>
#include <kernel/cmdline.h>
#include <kernel/mmio.h>
#include <kernel/print.h>
#include <kernel/utils.h>
#include <uacpi/acpi.h>
#include <uacpi/status.h>
#include <uacpi/tables.h>
#include <uacpi/uacpi.h>

static bool use_hpet = true;
static void hpet_option(bool use) {
    use_hpet = use;
}
CMDLINE_OPTION("hpet", hpet_option);

struct hpet {
    struct clock clock;
    volatile uint64_t* regs;
    uint32_t period;
};

#define HPET_CAP     (0x0 / sizeof(uint64_t))
#define HPET_CFG     (0x10 / sizeof(uint64_t))
#define HPET_COUNTER (0xF0 / sizeof(uint64_t))

static void hpet_reset(struct clock* c) {
    struct hpet* hpet = CONTAINER_OF(c, struct hpet, clock);
    hpet->regs[HPET_COUNTER] = 0;
}

static uint64_t hpet_get_elapsed_ns(struct clock* c) {
    struct hpet* hpet = CONTAINER_OF(c, struct hpet, clock);
    uint64_t counter = hpet->regs[HPET_COUNTER];
    return (counter * (uint64_t)hpet->period / 1'000'000);
}

static struct hpet hpet_clock = {
    .clock = {
        .name = "hpet",
        .priority = 128,
        .reset = hpet_reset,
        .get_elapsed_ns = hpet_get_elapsed_ns,
    },
};

static char table_buf[0x1000];

void hpet_init() {
    if (!use_hpet)
        return;

    uacpi_setup_early_table_access(table_buf, sizeof(table_buf));
    uacpi_table table = {0};
    ASSERT(!uacpi_table_find_by_signature("HPET", &table), "HPET table not found!\n");

    struct acpi_hpet* hpet = table.ptr;
    hpet_clock.regs = mmio_new(hpet->address.address, 0x1000);
    uacpi_table_unref(&table);
    kprintf("HPET regs at %p\n", hpet_clock.regs);

    hpet_clock.regs[HPET_CFG] = hpet_clock.regs[HPET_CFG] | 1;

    uint64_t caps = hpet_clock.regs[HPET_CAP];
    hpet_clock.period = (caps >> 32) & 0xFFFF'FFFF;

    ASSERT(clock_switch(&hpet_clock.clock), "Unable to switch to HPET!\n");

    uacpi_status status = uacpi_initialize(0);
    if (status != UACPI_STATUS_OK)
        return;
    status = uacpi_namespace_load();
    if (status != UACPI_STATUS_OK)
        return;
    uacpi_namespace_initialize();
}
