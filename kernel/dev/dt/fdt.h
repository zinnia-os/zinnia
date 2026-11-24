// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/types.h>
#include <stddef.h>
#include <stdint.h>

struct fdt_header {
    uint32_t magic;
    uint32_t total_size;
    uint32_t structs_offset;
    uint32_t strings_offset;
    uint32_t reserved_offset;
    uint32_t version;
    uint32_t compat_version;
    uint32_t boot_cpuid;
    uint32_t strings_size;
    uint32_t structs_size;
};

enum fdt_tag : uint32_t {
    FDT_BEGIN_NODE = 0x00000001,
    FDT_END_NODE = 0x00000002,
    FDT_PROP = 0x00000003,
    FDT_NOP = 0x00000004,
    FDT_END = 0x00000009,
    FDT_MAGIC = 0xD00DFEED,
};

struct fdt_match {
    const char* compatible;
    uintptr_t data;
};

struct fdt {
    const uint8_t* blob;
    size_t total_size;
    uint32_t version;
    uint32_t compat_version;
    uint32_t boot_cpuid;

    const uint8_t* structs;
    size_t structs_len;
    const uint8_t* strings;
    size_t strings_len;
};

struct fdt_node {
    struct fdt* tree;
    size_t offset;
};

struct fdt_prop {
    struct fdt* tree;
    const void* data;
    size_t length;
};

struct fdt* fdt_get();
struct fdt_node fdt_root(struct fdt* tree);
bool fdt_init(struct fdt* tree, const void* data, size_t length);
struct fdt_node fdt_find(struct fdt* tree, const char* path);
struct fdt_node fdt_find_compatible(struct fdt* tree, const char* compatible);
struct fdt_prop fdt_property(struct fdt_node node, const char* name);
bool fdt_property_u32(struct fdt_node node, const char* name, uint32_t* value);
bool fdt_property_u64(struct fdt_node node, const char* name, uint64_t* value);

struct fdt_node fdt_next_node(struct fdt_node node);
const char* fdt_node_name(struct fdt_node node);
bool fdt_node_is_compatible(struct fdt_node node, const char* compatible);
bool fdt_node_reg(struct fdt_node node, size_t index, phys_t* addr, size_t* size);

// Returns the index of `name` in the node's "reg-names" property.
// Returns -1 if it doesn't exist.
ssize_t fdt_node_reg_index(struct fdt_node node, const char* name);

// Maps the `index`th "reg" of a node as device memory.
errno_t fdt_node_map_reg(struct fdt_node node, size_t index, void** out, size_t* out_size);

struct fdt_driver {
    const char* name;

    const struct fdt_match* match;
    const size_t num_matches;

    errno_t (*probe)(struct fdt_node node, const struct fdt_match* match);
};

uint32_t fdt_read_u32(const void* data);
uint64_t fdt_read_u64(const void* data);

errno_t fdt_driver_register(struct fdt_driver* driver);

INIT_EXTERN(fdt_stage);
