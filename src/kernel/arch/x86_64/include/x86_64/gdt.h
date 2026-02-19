#pragma once

#include <common/compiler.h>
#include <stdint.h>

union gdt_desc {
    uint64_t val;
    struct {
        uint16_t limit0;
        uint16_t base0;
        uint8_t base1;
        uint8_t access;
        uint8_t limit1_flags;
        uint8_t base2;
    };
};

static_assert(sizeof(union gdt_desc) == 0x8);

union gdt_long_desc {
    uint64_t val[2];
    struct {
        uint16_t limit0;
        uint16_t base0;
        uint8_t base1;
        uint8_t access;
        uint8_t limit1_flags;
        uint8_t base2;
        uint32_t base3;
        uint32_t reserved;
    };
};

static_assert(sizeof(union gdt_long_desc) == 0x10);

struct [[__packed]] gdt {
    union gdt_desc null;
    union gdt_desc kernel_code32;
    union gdt_desc kernel_data32;
    union gdt_desc kernel_code64;
    union gdt_desc kernel_data64;
    union gdt_desc user_code32;
    union gdt_desc user_data;
    union gdt_desc user_code64;
    union gdt_long_desc tss;
};

struct [[__packed]] gdtr {
    uint16_t limit;
    struct gdt* base;
};

struct percpu;

// Initializes a GDT on the local CPU.
void gdt_load();
