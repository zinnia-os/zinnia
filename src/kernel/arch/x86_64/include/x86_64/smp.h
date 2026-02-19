#pragma once

#define SMP_GDTR_OFFSET       80
#define SMP_FARJMP_OFFSET     90
#define SMP_TEMP_STACK_OFFSET 98
#define SMP_TEMP_CR3_OFFSET   102
#define SMP_ENTRY_OFFSET      106
#define SMP_HHDM_OFFSET       114
#define SMP_KERNEL32_DS       16
#define SMP_KERNEL64_CS       24
#define SMP_KERNEL64_DS       32
#define SMP_INFO_SIZE         127

#ifndef __ASSEMBLER__

#include <common/compiler.h>
#include <common/utils.h>
#include <stddef.h>
#include <stdint.h>
#include <x86_64/gdt.h>

struct [[__packed]] smp_info {
    struct gdt gdt;
    struct gdtr gdtr;
    uint32_t farjmp_offset;
    uint32_t farjmp_segment;
    uint32_t temp_stack;
    uint32_t temp_cr3;
    uint64_t entry;
    uint64_t hhdm_offset;
    uint32_t lapic_id;
    uint8_t booted;
};

static_assert(SMP_GDTR_OFFSET == offsetof(struct smp_info, gdtr));
static_assert(SMP_FARJMP_OFFSET == offsetof(struct smp_info, farjmp_offset));
static_assert(SMP_TEMP_STACK_OFFSET == offsetof(struct smp_info, temp_stack));
static_assert(SMP_TEMP_CR3_OFFSET == offsetof(struct smp_info, temp_cr3));
static_assert(SMP_ENTRY_OFFSET == offsetof(struct smp_info, entry));
static_assert(SMP_HHDM_OFFSET == offsetof(struct smp_info, hhdm_offset));
static_assert(SMP_KERNEL32_DS == offsetof(struct gdt, kernel_data32));
static_assert(SMP_KERNEL64_CS == offsetof(struct gdt, kernel_code64));
static_assert(SMP_KERNEL64_DS == offsetof(struct gdt, kernel_data64));
static_assert(SMP_INFO_SIZE == sizeof(struct smp_info));

// Initializes a LAPIC.
void x86_64_smp_init(uint32_t id);

#endif
