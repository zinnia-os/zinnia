#pragma once

#include <kernel/irq.h>
#include <stddef.h>
#include <x86_64/apic.h>
#include <x86_64/gdt.h>
#include <x86_64/tss.h>

static inline struct percpu* arch_percpu_get() {
    struct percpu* result;
    asm volatile("mov %0, gs:0" : "=r"(result)::"memory");
    return result;
}

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

void arch_percpu_bsp_init();
void arch_percpu_init();
