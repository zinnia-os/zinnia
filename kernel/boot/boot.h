// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/types.h>
#include <stddef.h>
#include <stdint.h>

enum phys_mem_usage {
    PHYS_RESERVED,
    PHYS_USABLE,
    PHYS_STATIC,
    PHYS_RECLAIMABLE,
};

struct phys_mem {
    phys_t address;
    size_t length;
    enum phys_mem_usage usage;
};

constexpr size_t BOOT_PATH_MAX = 128;

struct boot_file {
    phys_t data;
    size_t length;
    char path[BOOT_PATH_MAX];
};

struct boot_color {
    uint8_t shift, size;
};

struct boot_framebuffer {
    phys_t base;
    size_t width, height, pitch;
    uint8_t bytes_per_pixel;
    struct boot_color red, green, blue;
};

constexpr size_t BOOT_LOADER_NAME_MAX = 64;
constexpr size_t BOOT_CMDLINE_MAX = 4096;
constexpr size_t BOOT_MEM_MAP_MAX = 128;
constexpr size_t BOOT_FILES_MAX = 8;

struct boot_info {
    char loader[BOOT_LOADER_NAME_MAX];
    char cmdline[BOOT_CMDLINE_MAX];
    struct phys_mem mem_map[BOOT_MEM_MAP_MAX];
    size_t num_mem_maps;
    phys_t phys_base;
    uintptr_t virt_base;
    uintptr_t hhdm_base;
    struct boot_file files[BOOT_FILES_MAX];
    size_t num_files;
    struct boot_framebuffer fb;
    phys_t dtb;
    size_t dtb_size;
    phys_t rsdp;
};
