#pragma once

#include <menix/status.h>
#include <bits/mem.h>
#include <kernel/compiler.h>
#include <kernel/spin.h>
#include <kernel/types.h>
#include <stddef.h>

ASSERT_TYPE(pte_t);

enum pte_flags {
    PTE_READ = 1 << 0,  // Can read from this page.
    PTE_WRITE = 1 << 1, // Can write to this page.
    PTE_EXEC = 1 << 2,  // Can execute code on this page.
    PTE_USER = 1 << 3,  // Can be accessed by the user.
    PTE_DIR = 1 << 4,   // Is a non-leaf page.
};

enum cache_mode {
    CACHE_NONE,
    CACHE_WRITE_COMBINE,
    CACHE_WRITE_THROUGH,
    CACHE_WRITE_BACK,
    CACHE_MMIO,
};

struct page_table {
    phys_t root;
    struct spinlock lock;
};

enum alloc_flags {
    ALLOC_NOZERO = 1 << 0, // Don't zero out the allocated memory.
    ALLOC_MEM32 = 1 << 1,  // Allocated memory needs to fit inside 32 bits.
    ALLOC_MEM20 = 1 << 2,  // Allocated memory needs to fit inside 20 bits.
};

enum phys_mem_usage {
    PHYS_RESERVED,
    PHYS_USABLE,
    PHYS_RECLAIMABLE,
};

struct phys_mem {
    phys_t address;
    size_t length;
    enum phys_mem_usage usage;
};

enum page_type {
    PAGE_PHYS = 0, // Regular physical memory.
};

struct page {
    enum page_type type;
    int32_t flags;
    size_t refcount;
    union {
        struct {
            struct page* next; // Pointer to the next chunk.
            size_t count;      // Amount of free pages.
        } freelist;
    };
};
static_assert(0x1000 % sizeof(struct page) == 0, "struct must be a multiple of the page size!");
static_assert(sizeof(struct page) <= 64, "struct must be smaller than 64 bytes!");

struct address_space {
    // TODO
};

extern struct page_table mem_kernel_table;

// Base address of the `struct page` array.
extern struct page* mem_pfndb;

// Base address of the HHDM.
extern virt_t mem_hhdm_base;
#define HHDM_PTR(paddr) (void*)((paddr) + mem_hhdm_base)

void mem_init(struct phys_mem* map, size_t length, virt_t kernel_virt, phys_t kernel_phys, virt_t tmp_hhdm);

void mem_phys_bootstrap(struct phys_mem* mem);
void mem_phys_init(struct phys_mem* map, size_t length);

// Allocates a region of memory which can be smaller than the page size.
// Returns `nullptr` if the allocator cannot provide an allocation for the
// given `length` + `flags`. Always returns `nullptr` if `length` is 0.
menix_status_t mem_alloc(size_t length, enum alloc_flags flags, void** out);

// Frees an allocation created by `mem_alloc`.
// Passing `nullptr` is a no-op.
menix_status_t mem_free(void* mem);

// Allocates an amount of contiguous physical pages.
menix_status_t mem_phys_alloc(size_t num_pages, enum alloc_flags flags, phys_t* out);

// Frees an allocated region of physical pages.
menix_status_t mem_phys_free(phys_t start, size_t num_pages);

// Creates a new page table for the kernel.
menix_status_t mem_pt_new_kernel(struct page_table* pt, enum alloc_flags flags);

// Creates a new page table for a user process.
menix_status_t mem_pt_new_user(struct page_table* pt, enum alloc_flags flags);

// Sets a page table on the current processor.
static inline void mem_pt_set(struct page_table* pt) {
    arch_mem_pt_set(pt);
}

// Maps a single page to a virtual address in the given page table.
menix_status_t mem_pt_map(
    struct page_table* pt,
    virt_t vaddr,
    phys_t paddr,
    enum pte_flags flags,
    enum cache_mode cache
);

// Changes the protection of a page.
menix_status_t mem_pt_protect(struct page_table* pt, virt_t vaddr, enum pte_flags flags);

// Unmaps a page.
menix_status_t mem_pt_unmap(struct page_table* pt, virt_t vaddr);

// Base address used to access physical memory.
static inline virt_t mem_hhdm_addr() {
    return arch_mem_hhdm_addr();
}

// Base address used to access the page array.
static inline virt_t mem_pfndb_addr() {
    return arch_mem_pfndb_addr();
}

// Base address used to access kernel memory mappings.
static inline virt_t mem_mapping_addr() {
    return arch_mem_mapping_addr();
}

// Bit shift in a page.
static inline size_t mem_page_bits() {
    return arch_mem_page_bits();
}

// Gets the page size in bytes.
static inline size_t mem_page_size() {
    return 1 << mem_page_bits();
}

// The amount of bits in a level.
static inline size_t mem_level_bits() {
    return arch_mem_level_bits();
}

// The amount of levels in an address.
static inline size_t mem_num_levels() {
    return arch_mem_num_levels();
}

// Clears out a page table entry.
static inline void mem_pte_clear(pte_t* pte) {
    return arch_mem_pte_clear(pte);
}

// Builds a page table entry from the given info.
static inline pte_t mem_pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache) {
    return arch_mem_pte_build(addr, flags, cache);
}

// Returns true if the given PTE is present and valid.
static inline bool mem_pte_is_present(pte_t* pte) {
    return arch_mem_pte_is_present(pte);
}

// Returns true if the given PTE contains another level.
static inline bool mem_pte_is_dir(pte_t* pte) {
    return arch_mem_pte_is_dir(pte);
}

// Returns the address component of the page table entry.
static inline phys_t mem_pte_address(pte_t* pte) {
    return arch_mem_pte_address(pte);
}
