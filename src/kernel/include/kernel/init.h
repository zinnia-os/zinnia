#pragma once

#include <common/compiler.h>
#include <common/utils.h>
#include <kernel/types.h>
#include <stddef.h>
#include <stdint.h>

#define __init                  __used, __section(".init.text"), __cold
#define __initdata              __used, __section(".init.data")
#define __initdata_sorted(name) __used, __section(".init.data." name)

struct boot_file {
    phys_t data;
    size_t length;
    char path[128];
};

struct boot_info {
    char* cmdline;
    struct phys_mem* mem_map;
    size_t num_mem_maps;
    phys_t phys_base;
    uintptr_t virt_base;
    uintptr_t hhdm_base;
    struct boot_file* files;
    size_t num_files;
    phys_t rsdp;
};

extern uint8_t __ld_stack_top[];
extern uint8_t __ld_stack_bottom[];

extern phys_t rsdp_addr;

// Entry point for the kernel after arch-specific setup has finished.
void kernel_entry();

// Initializes the early parts of the kernel.
void kernel_early_init();

[[noreturn]]
void kernel_main(struct boot_info* info);
