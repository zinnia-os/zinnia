#pragma once

#include <common/compiler.h>
#include <stdint.h>
#include <x86_64/tss.h>

struct [[__packed]] gdt {
    uint64_t null;
    uint64_t kernel_code32;
    uint64_t kernel_data32;
    uint64_t kernel_code64;
    uint64_t kernel_data64;
    uint64_t user_code;
    uint64_t user_data;
    uint64_t user_code64;
    uint64_t tss[2];
};

struct [[__packed]] gdtr {
    uint16_t limit;
    struct gdt* base;
};

struct percpu;

// Initializes a GDT on the local CPU.
void gdt_load();

// Sets the Task State Segment in the given GDT.
void gdt_set_tss(struct gdt* gdt, struct tss* tss);
