#pragma once

#include <kernel/types.h>
#include <uapi/errno.h>
#include <stddef.h>

enum alloc_flags {
    ALLOC_NOZERO = 1 << 0, // Don't zero out the allocated memory.
    ALLOC_MEM32 = 1 << 1,  // Allocated memory needs to fit inside 32 bits.
    ALLOC_MEM20 = 1 << 2,  // Allocated memory needs to fit inside 20 bits.
};

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

// Base address of the HHDM.
extern uintptr_t mem_hhdm_base;
#define HHDM_PTR(paddr) (void*)(mem_hhdm_base + (paddr))

// Allocates a region of memory which can be smaller than the page size.
// Returns `nullptr` if the allocator cannot provide an allocation for the
// given `length` + `flags`. Always returns `nullptr` if `length` is 0.
void* mem_alloc(size_t length, enum alloc_flags flags);

// Frees an allocation created by `mem_alloc`.
// Passing `nullptr` is a no-op.
void mem_free(void* mem);

errno_t mem_phys_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out);
errno_t mem_phys_free(phys_t start, size_t num_pages);

void mem_init(struct phys_mem* map, size_t length, uintptr_t kernel_virt, phys_t kernel_phys, uintptr_t tmp_hhdm);
void slab_init();
void mem_phys_bootstrap(struct phys_mem* mem);
void mem_phys_init(struct phys_mem* map, size_t length);

uintptr_t arch_mem_hhdm_addr();
uintptr_t arch_vm_pfndb_addr();
uintptr_t arch_mem_mapping_addr();
size_t arch_mem_page_bits();
size_t arch_mem_page_size();
size_t arch_mem_level_bits();
size_t arch_mem_num_levels();

static inline size_t mem_high_shift() {
    return arch_mem_level_bits() * arch_mem_num_levels() + arch_mem_page_bits();
}
