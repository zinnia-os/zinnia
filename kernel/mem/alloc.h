// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <boot/boot.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/types.h>
#include <stddef.h>

enum alloc_flags {
    // Don't zero out the allocated memory.
    ALLOC_NOZERO = 1 << 0,
    // Allocated memory needs to fit inside 32 bits.
    ALLOC_MEM32 = 1 << 1,
    // Allocated memory needs to fit inside 20 bits.
    ALLOC_MEM20 = 1 << 2,
    // Don't block while waiting for memory.
    ALLOC_NOSLEEP = 1 << 3,
};

// Allocates a region of memory of arbitrary size.
void* mem_alloc(size_t size, enum alloc_flags flags);

// Frees an allocation created by `mem_alloc`.
// `size` must be the same size that was passed to `mem_alloc`.
// Passing `nullptr` is a no-op.
void mem_free(void* buf, size_t size);

errno_t mem_phys_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out);
errno_t mem_phys_free(phys_t start, size_t num_pages);

struct page;

uintptr_t vm_alloc(size_t length);
void vm_free(uintptr_t addr, size_t length);

struct mem_cache;

enum mem_cache_flags {
    // Create a cache without using a magazine.
    MEM_CACHE_NOMAGAZINE = 1 << 0,
};

struct mem_cache* mem_cache_create(const char* name, size_t size, size_t align, enum mem_cache_flags flags);
void mem_cache_destroy(struct mem_cache* cache);
void* mem_cache_alloc(struct mem_cache* cache, enum alloc_flags flags);
void mem_cache_free(struct mem_cache* cache, void* buf);

size_t mem_cache_num_slabs(struct mem_cache* cache);
size_t mem_cache_magsize(struct mem_cache* cache);
void mem_cache_magazine_resize(struct mem_cache* cache);

INIT_EXTERN(mem_init_stage);
INIT_EXTERN(mem_phys_stage);
INIT_EXTERN(mem_cache_stage);
INIT_EXTERN(mem_stage);
