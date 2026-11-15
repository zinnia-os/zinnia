// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/irq.h>
#include <lib/mmio.h>
#include <stdint.h>

struct local_apic {
    uint32_t id;
    uint32_t ticks_per_10ms;
    void* xapic_regs;
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

constexpr mmio_reg32 APIC_REG_ID = {.offset = 0x20};
constexpr mmio_field32 APIC_ID_XAPIC = {APIC_REG_ID, 24, 8};
constexpr mmio_reg32 APIC_REG_TPR = {.offset = 0x80};
constexpr mmio_reg32 APIC_REG_EOI = {.offset = 0xB0};
constexpr mmio_reg32 APIC_REG_LDR = {.offset = 0xD0};
constexpr mmio_reg32 APIC_REG_DFR = {.offset = 0xE0};
constexpr mmio_reg32 APIC_REG_SIVR = {.offset = 0xF0};
constexpr mmio_field32 APIC_SIVR_VECTOR = {APIC_REG_SIVR, 0, 8};
constexpr mmio_field32 APIC_SIVR_ENABLE = {APIC_REG_SIVR, 8, 1};
constexpr mmio_reg32 APIC_REG_ESR = {.offset = 0x280};
constexpr mmio_reg64 APIC_REG_ICR = {.offset = 0x300};
constexpr mmio_field64 APIC_ICR_VECTOR = {APIC_REG_ICR, 0, 8};
constexpr mmio_field64 APIC_ICR_DELIVERY_MODE = {APIC_REG_ICR, 8, 3};
constexpr mmio_field64 APIC_ICR_DEST_MODE = {APIC_REG_ICR, 11, 1};
constexpr mmio_field64 APIC_ICR_DELIVERY_STATUS = {APIC_REG_ICR, 12, 1};
constexpr mmio_field64 APIC_ICR_LEVEL = {APIC_REG_ICR, 14, 1};
constexpr mmio_field64 APIC_ICR_TRIGGER_MODE = {APIC_REG_ICR, 15, 1};
constexpr mmio_field64 APIC_ICR_DEST_XAPIC = {APIC_REG_ICR, 56, 8};
constexpr mmio_field64 APIC_ICR_DEST_X2APIC = {APIC_REG_ICR, 32, 32};
constexpr mmio_reg32 APIC_REG_ICR_LO = {.offset = 0x300};
constexpr mmio_reg32 APIC_REG_ICR_HI = {.offset = 0x310};
constexpr mmio_reg32 APIC_REG_LVT_TR = {.offset = 0x320};
constexpr mmio_field32 APIC_LVT_TR_VECTOR = {APIC_REG_LVT_TR, 0, 8};
constexpr mmio_reg32 APIC_REG_ICR_TIMER = {.offset = 0x380};
constexpr mmio_reg32 APIC_REG_CCR = {.offset = 0x390};
constexpr mmio_reg32 APIC_REG_DCR = {.offset = 0x3E0};

void lapic_init(struct local_apic* lapic);
void lapic_eoi(struct local_apic* lapic);

void lapic_send_ipi(
    struct local_apic* lapic,
    uint32_t target,
    uint8_t vector,
    enum apic_delivery_mode delivery_mode,
    enum apic_dest_mode dest_mode,
    enum apic_level level,
    enum apic_trigger_mode trigger_mode
);

void pic_disable();

errno_t apic_alloc_vector(struct irq_line* line, uint8_t* out_vector, uint32_t* out_lapic);
void apic_free_vector(uint8_t vector);

INIT_EXTERN(ioapic_stage);
