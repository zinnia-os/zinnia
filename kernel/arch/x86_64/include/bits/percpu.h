#pragma once

#include <kernel/irq.h>
#include <stddef.h>
#include <x86_64/apic.h>
#include <x86_64/gdt.h>
#include <x86_64/tss.h>

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
