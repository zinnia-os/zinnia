// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <boot/boot.h>
#include <lib/utils.h>
#include <stddef.h>
#include <stdint.h>

constexpr uint64_t PAGE_SIZE = 0x1000;

struct kernel_image {
    uint64_t phys;
    uint64_t virt;
    uint64_t size;
    uint64_t entry;
    struct boot_info* info;
};

bool loader_alloc(enum phys_mem_usage usage, size_t size, uint64_t* out);
void* loader_ptr(uint64_t phys);

bool elf_load(const uint8_t* elf, size_t size, struct kernel_image* out);
void* kernel_ptr(const struct kernel_image* image, uint64_t vaddr);

bool loader_load_kernel(struct kernel_image* out, const char* name);
bool loader_add_file(const struct kernel_image* kernel, const char* path, uint64_t base, uint64_t length);
void loader_set_mem_map(const struct kernel_image* kernel, struct phys_mem* map, size_t len);

bool loader_page_tables(
    const struct kernel_image* kernel,
    uint64_t mem_top,
    uint64_t trampoline_phys,
    uint64_t* out_root
);

bool arch_check_cpu();
bool arch_paging_map(uint64_t root, uint64_t virt, uint64_t phys, uint64_t size, uint64_t page_size);

[[noreturn]]
void arch_enter_kernel(uint64_t root, uint64_t entry);
