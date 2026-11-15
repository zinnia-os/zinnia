// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/x86_64/gdt.h>
#include <lib/atomic.h>
#include <lib/compiler.h>
#include <lib/utils.h>
#include <stdint.h>

struct [[__packed]] smp_info {
    struct gdt gdt;
    struct gdtr gdtr;
    uint32_t farjmp_offset;
    uint32_t farjmp_segment;
    uint32_t temp_stack;
    uint32_t temp_cr3;
    uint64_t entry;
    uint64_t hhdm_offset;
    uint64_t percpu;
    uint32_t lapic_id;
    atomic_bool booted;
};

// Discovers all APs and starts them.
void x86_64_smp_init();
