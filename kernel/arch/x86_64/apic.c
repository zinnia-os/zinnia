#include <kernel/assert.h>
#include <kernel/clock.h>
#include <kernel/mmio.h>
#include <kernel/percpu.h>
#include <stdint.h>
#include <x86_64/apic.h>
#include <x86_64/asm.h>
#include <x86_64/defs.h>

static uint64_t lapic_read_reg(struct local_apic* lapic, uint32_t reg) {
    if (lapic->xapic_regs) {
        if (reg == APIC_REG_ICR) {
            uint64_t lo, hi;
            lo = lapic->xapic_regs[APIC_REG_ICR / sizeof(uint32_t)];
            hi = lapic->xapic_regs[APIC_REG_ICR_HI / sizeof(uint32_t)];
            return (hi << 32) | lo;
        } else {
            return lapic->xapic_regs[reg / sizeof(uint32_t)];
        }
    } else {
        return asm_rdmsr(0x800 + (reg >> 4));
    }
}

static void lapic_write_reg(struct local_apic* lapic, uint32_t reg, uint64_t value) {
    if (lapic->xapic_regs) {
        if (reg == APIC_REG_ICR) {
            lapic->xapic_regs[APIC_REG_ICR_HI / sizeof(uint32_t)] = value >> 32;
            lapic->xapic_regs[APIC_REG_ICR / sizeof(uint32_t)] = value & 0xFFFF'FFFF;
        } else {
            lapic->xapic_regs[reg / sizeof(uint32_t)] = value & 0xFFFF'FFFF;
        }
    } else {
        asm_wrmsr(0x800 + (reg >> 4), value);
    }
}

void lapic_init(struct local_apic* lapic) {
    uint64_t apic_msr = asm_rdmsr(0x1B);
    apic_msr |= 1 << 11; // Enable APIC.

    struct cpuid cpuid = asm_cpuid(1, 0);
    if (cpuid.ecx & CPUID_1C_X2APIC)
        apic_msr |= 1 << 10;
    else {
        lapic->xapic_regs = mmio_new(apic_msr & 0xFFFF'F000, 0x1000);
        ASSERT(lapic->xapic_regs, "Failed to allocate virtual memory!\n");
    }

    asm_wrmsr(0x1B, apic_msr);

    // Reset the TPR.
    lapic_write_reg(lapic, APIC_REG_TPR, 0);
    // Enable APIC bit in the SIVR.
    lapic_write_reg(lapic, APIC_REG_SIVR, lapic_read_reg(lapic, APIC_REG_SIVR) | 0x100);

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
    lapic_write_reg(lapic, APIC_REG_LVT_TR, IDT_IPI_RESCHED | 0x20000);
    lapic_write_reg(lapic, APIC_REG_ICR_TIMER, lapic->ticks_per_10ms);
}

void lapic_eoi(struct local_apic* lapic) {
    lapic_write_reg(lapic, APIC_REG_EOI, 0);
}

#define PIC1_COMMAND_PORT 0x20
#define PIC1_DATA_PORT    0x21
#define PIC2_COMMAND_PORT 0xA0
#define PIC2_DATA_PORT    0xA1

void pic_disable() {
    // Note: We initialize the PIC properly, but completely disable it and use the APIC in favor of it.
    // Remap IRQs so they start at 0x20 since interrupts 0x00..0x1F are used by CPU exceptions.
    asm_outb(PIC1_COMMAND_PORT, 0x11); // ICW1: Begin initialization and set cascade mode.
    asm_outb(PIC1_DATA_PORT, 0x20);    // ICW2: Set where interrupts should be mapped to (0x20-0x27).
    asm_outb(PIC1_DATA_PORT, 0x04);    // ICW3: Connect IRQ2 (0x04) to the slave PIC.
    asm_outb(PIC1_DATA_PORT, 0x01);    // ICW4: Set the PIC to operate in 8086/88 mode.
    asm_outb(PIC1_DATA_PORT, 0xFF);    // Mask all interrupts.

    // Same for the slave PIC.
    asm_outb(PIC2_COMMAND_PORT, 0x11); // ICW1: Begin initialization.
    asm_outb(PIC2_DATA_PORT, 0x28);    // ICW2: Set where interrupts should be mapped to (0x28-0x2F).
    asm_outb(PIC2_DATA_PORT, 0x02);    // ICW3: Connect to master PIC at IRQ2.
    asm_outb(PIC2_DATA_PORT, 0x01);    // ICW4: Set the PIC to operate in 8086/88 mode.
    asm_outb(PIC2_DATA_PORT, 0xFF);    // Mask all interrupts.
}
