// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/x86_64/apic.h>
#include <arch/x86_64/gdt.h>
#include <arch/x86_64/tss.h>
#include <lib/irq.h>
#include <lib/percpu.h>
#include <stddef.h>

struct arch_percpu {
    struct gdt gdt;
    struct tss tss;
    struct local_apic lapic;
    struct irq_line* irq_lines[128];
    size_t fpu_size;
    void (*fpu_save)(void*);
    void (*fpu_restore)(const void*);
    bool can_smap;
};

extern struct arch_percpu arch_data;

void x86_64_cpu_setup(struct percpu* cpu);
