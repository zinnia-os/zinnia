#pragma once

#include <kernel/compiler.h>
#include <kernel/irq.h>
#include <kernel/sched.h>
#include <kernel/usercopy.h>
#include <bits/percpu.h>
#include <stddef.h>

ASSERT_TYPE(struct arch_percpu);

// CPU-relative information.
struct percpu {
    struct percpu* self;       // A pointer to this structure.
    size_t id;                 // The virtual ID of this CPU.
    bool online;               // Whether this CPU is initialized and active.
    uintptr_t kernel_stack;    // The kernel mode stack.
    uintptr_t user_stack;      // The user mode stack.
    struct arch_percpu arch;   // Architecture-specific fields.
    struct irq_percpu irq;     // IRQ information.
    struct sched_percpu sched; // Scheduler information.
    struct usercopy_region* usercopy_region;
};

// Per-CPU data for the bootstrap processor.
extern struct percpu percpu_bsp;

// Allocates a block of memory for a new CPU.
struct percpu* percpu_new();

// Gets the per-CPU data on the current CPU.
struct percpu* percpu_get();

// Initializes the bootstrap processor.
void percpu_bsp_init();

// Initializes all processors.
void percpu_init();
