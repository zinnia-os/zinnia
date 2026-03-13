#pragma once

#include <stdint.h>

struct local_apic {
    uint32_t id;
    uint32_t ticks_per_10ms;
    volatile uint32_t* xapic_regs;
};

enum apic_delivery_mode {
    APIC_DELIVERY_FIXED = 0b000,
    APIC_DELIVERY_LOWESTPRIO = 0b001,
    APIC_DELIVERY_SMI = 0b010,
    APIC_DELIVERY_NMI = 0b100,
    APIC_DELIVERY_INIT = 0b101,
    APIC_DELIVERY_STARTUP = 0b110,
};

enum apic_dest_mode {
    APIC_DEST_PHYSICAL = 0,
    APIC_DEST_LOGICAL = 1,
};

enum apic_delivery_status {
    APIC_DELIVERY_STATUS_IDLE = 0,
    APIC_DELIVERY_STATUS_PENDING = 1,
};

enum apic_level {
    APIC_LEVEL_DEASSERT = 0,
    APIC_LEVEL_ASSERT = 1,
};

enum apic_trigger_mode {
    APIC_TRIGGER_EDGE = 0,
    APIC_TRIGGER_LEVEL = 1,
};

enum apic_regs {
    APIC_REG_ID = 0x20,
    APIC_REG_TPR = 0x80,
    APIC_REG_EOI = 0xB0,
    APIC_REG_LDR = 0xD0,
    APIC_REG_DFR = 0xE0,
    APIC_REG_SIVR = 0xF0,
    APIC_REG_ESR = 0x280,
    APIC_REG_ICR = 0x300,
    APIC_REG_ICR_HI = 0x310,
    APIC_REG_LVT_TR = 0x320,
    APIC_REG_ICR_TIMER = 0x380,
    APIC_REG_CCR = 0x390,
    APIC_REG_DCR = 0x3E0,
};

void lapic_init(struct local_apic* lapic);
void lapic_eoi(struct local_apic* lapic);
void pic_disable();
