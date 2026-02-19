#pragma once

#include <zinnia/status.h>
#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/spin.h>
#include <bits/mmu.h>

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
    bool is_user;
};

zn_status_t pt_new_kernel(struct page_table* pt, enum alloc_flags flags);
zn_status_t pt_new_user(struct page_table* pt, enum alloc_flags flags);
zn_status_t pt_map(struct page_table* pt, uintptr_t vaddr, phys_t paddr, enum pte_flags flags, enum cache_mode cache);
zn_status_t pt_protect(struct page_table* pt, uintptr_t vaddr, enum pte_flags flags);
zn_status_t pt_unmap(struct page_table* pt, uintptr_t vaddr);
void pt_set(struct page_table* pt);

void pte_clear(pte_t* pte);
pte_t pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache);
bool pte_is_present(pte_t* pte);
bool pte_is_dir(pte_t* pte);
phys_t pte_address(pte_t* pte);
