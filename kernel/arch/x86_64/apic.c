// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/apic.h>
#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/percpu.h>
#include <lib/assert.h>
#include <lib/clock.h>
#include <lib/cpu.h>
#include <lib/mmio.h>
#include <lib/percpu.h>
#include <lib/timer.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stdint.h>

static uint32_t lapic_msr(size_t offset) {
    return 0x800 + (offset >> 4);
}

static uint32_t lapic_read_reg(struct local_apic* lapic, mmio_reg32 reg) {
    if (lapic->xapic_regs)
        return mmio_reg_read(lapic->xapic_regs, reg);
    else
        return asm_rdmsr(lapic_msr(reg.offset));
}

static void lapic_write_reg(struct local_apic* lapic, mmio_reg32 reg, uint32_t value) {
    if (lapic->xapic_regs)
        mmio_reg_write(lapic->xapic_regs, reg, value);
    else
        asm_wrmsr(lapic_msr(reg.offset), value);
}

static uint64_t lapic_read_icr(struct local_apic* lapic) {
    if (lapic->xapic_regs) {
        uint64_t lo = mmio_reg_read(lapic->xapic_regs, APIC_REG_ICR_LO);
        uint64_t hi = mmio_reg_read(lapic->xapic_regs, APIC_REG_ICR_HI);
        return (hi << 32) | lo;
    } else {
        return asm_rdmsr(lapic_msr(APIC_REG_ICR.offset));
    }
}

static void lapic_write_icr(struct local_apic* lapic, uint64_t value) {
    if (lapic->xapic_regs) {
        mmio_reg_write(lapic->xapic_regs, APIC_REG_ICR_HI, (uint32_t)(value >> 32));
        mmio_reg_write(lapic->xapic_regs, APIC_REG_ICR_LO, (uint32_t)value);
    } else {
        asm_wrmsr(lapic_msr(APIC_REG_ICR.offset), value);
    }
}

void lapic_init(struct local_apic* lapic) {
    uint64_t apic_msr = asm_rdmsr(0x1B);
    apic_msr |= 1 << 11; // Enable APIC.

    struct cpuid cpuid = asm_cpuid(1, 0);
    if (cpuid.ecx & CPUID_1C_X2APIC)
        apic_msr |= 1 << 10;
    else {
        uintptr_t vaddr = vm_alloc(0x1000);
        pmap_map(&kernel_vma.pmap, vaddr, apic_msr & 0xFFFF'F000, PTE_READ | PTE_WRITE, CACHE_MMIO);
        lapic->xapic_regs = (void*)vaddr;
        assert(lapic->xapic_regs, "Failed to allocate virtual memory!");
    }

    asm_wrmsr(0x1B, apic_msr);

    lapic->id = lapic_read_reg(lapic, APIC_REG_ID);
    if (lapic->xapic_regs)
        lapic->id = mmio_field_get(APIC_ID_XAPIC, lapic->id);

    // Reset the TPR.
    lapic_write_reg(lapic, APIC_REG_TPR, 0);
    // Enable APIC bit in the SIVR.
    uint32_t sivr = lapic_read_reg(lapic, APIC_REG_SIVR);
    sivr |= mmio_field(APIC_SIVR_VECTOR, IDT_SPURIOUS);
    sivr |= mmio_field(APIC_SIVR_ENABLE, 1);
    lapic_write_reg(lapic, APIC_REG_SIVR, sivr);

    if (lapic->xapic_regs) {
        lapic_write_reg(lapic, APIC_REG_DFR, 0xF000'0000);
        // Logical destination = LAPIC ID.
        lapic_write_reg(lapic, APIC_REG_LDR, lapic_read_reg(lapic, APIC_REG_ID));
    }

    // TODO: Parse MADT and setup NMI sources.

    // Tell the APIC timer to divide by 16.
    lapic_write_reg(lapic, APIC_REG_DCR, 3);
    // Set the timer to the highest value possible.
    lapic_write_reg(lapic, APIC_REG_ICR_TIMER, 0xFFFF'FFFF);

    // Sleep for 10 milliseconds.
    clock_spin_ns(10'000'000);

    lapic->ticks_per_10ms = 0xFFFF'FFFF - lapic_read_reg(lapic, APIC_REG_CCR);
    lapic_write_reg(lapic, APIC_REG_LVT_TR, mmio_field(APIC_LVT_TR_VECTOR, IDT_TIMER));
    lapic_write_reg(lapic, APIC_REG_ICR_TIMER, 0);
}

void arch_timer_program(uint64_t deadline_ns) {
    struct local_apic* lapic = &percpu_ref(arch_data)->lapic;
    const uint64_t now = clock_get_elapsed_ns();
    const uint64_t delta = MIN(deadline_ns > now ? deadline_ns - now : 0, 1'000'000'000);
    const uint64_t ticks = delta * lapic->ticks_per_10ms / 10'000'000;
    lapic_write_reg(lapic, APIC_REG_ICR_TIMER, MIN(MAX(ticks, 1), 0xFFFF'FFFF));
}

void lapic_eoi(struct local_apic* lapic) {
    lapic_write_reg(lapic, APIC_REG_EOI, 0);
}

void lapic_send_ipi(
    struct local_apic* lapic,
    uint32_t target,
    uint8_t vector,
    enum apic_delivery_mode delivery_mode,
    enum apic_dest_mode dest_mode,
    enum apic_level level,
    enum apic_trigger_mode trigger_mode
) {
    uint64_t icr = mmio_field(APIC_ICR_VECTOR, vector);
    icr |= mmio_field(APIC_ICR_DELIVERY_MODE, delivery_mode);
    icr |= mmio_field(APIC_ICR_DEST_MODE, dest_mode);
    icr |= mmio_field(APIC_ICR_LEVEL, level);
    icr |= mmio_field(APIC_ICR_TRIGGER_MODE, trigger_mode);

    if (lapic->xapic_regs)
        icr |= mmio_field(APIC_ICR_DEST_XAPIC, target);
    else
        icr |= mmio_field(APIC_ICR_DEST_X2APIC, target);

    lapic_write_icr(lapic, icr);

    if (lapic->xapic_regs) {
        while (mmio_field_get(APIC_ICR_DELIVERY_STATUS, lapic_read_icr(lapic)) == APIC_DELIVERY_STATUS_PENDING)
            arch_cpu_pause();
    }
}

constexpr uint16_t PIC1_COMMAND_PORT = 0x20;
constexpr uint16_t PIC1_DATA_PORT = 0x21;
constexpr uint16_t PIC2_COMMAND_PORT = 0xA0;
constexpr uint16_t PIC2_DATA_PORT = 0xA1;

void pic_disable() {
    // We initialize the PIC properly, but completely disable it and use the APIC in favor of it.
    // Remap IRQs so they start at 0x20 since interrupts 0x00..0x1F are used by CPU exceptions.
    asm_outb(PIC1_COMMAND_PORT, 0x11);
    asm_outb(PIC1_DATA_PORT, 0x20);
    asm_outb(PIC1_DATA_PORT, 0x04);
    asm_outb(PIC1_DATA_PORT, 0x01);
    asm_outb(PIC1_DATA_PORT, 0xFF);

    // Same for the slave PIC.
    asm_outb(PIC2_COMMAND_PORT, 0x11);
    asm_outb(PIC2_DATA_PORT, 0x28);
    asm_outb(PIC2_DATA_PORT, 0x02);
    asm_outb(PIC2_DATA_PORT, 0x01);
    asm_outb(PIC2_DATA_PORT, 0xFF);
}
