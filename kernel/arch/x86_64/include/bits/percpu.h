#pragma once

#include <gdt.h>
#include <stdint.h>

static inline struct percpu* arch_percpu_get() {
    struct percpu* result;
    asm volatile("mov %0, gs:0" : "=r"(result)::"memory");
    return result;
}

struct arch_percpu {
    uint32_t lapic_id;
    struct gdt gdt;
    struct tss tss;
};
