// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/spin.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <config.h>

struct percpu;

enum pte_flags : uint8_t {
    PTE_READ = 1 << 0,  // Can read from this page.
    PTE_WRITE = 1 << 1, // Can write to this page.
    PTE_EXEC = 1 << 2,  // Can execute code on this page.
    PTE_USER = 1 << 3,  // Can be accessed by the user.
    PTE_DIR = 1 << 4,   // Is a non-leaf page.
};

enum cache_mode : uint8_t {
    CACHE_ANY,
    CACHE_WRITE_COMBINE,
    CACHE_WRITE_THROUGH,
    CACHE_WRITE_BACK,
    CACHE_MMIO,
};

struct pmap {
    phys_t root;
    struct spinlock lock;
    bool is_user;
};

errno_t pmap_new_kernel(struct pmap* pt, enum alloc_flags flags);
errno_t pmap_new_user(struct pmap* pt, enum alloc_flags flags);
errno_t pmap_map(struct pmap* pt, uintptr_t vaddr, phys_t paddr, enum pte_flags flags, enum cache_mode cache);
errno_t pmap_protect(struct pmap* pt, uintptr_t vaddr, enum pte_flags flags);
errno_t pmap_unmap(struct pmap* pt, uintptr_t vaddr);
errno_t pmap_extract(struct pmap* pt, uintptr_t vaddr, phys_t* out);
void pmap_destroy(struct pmap* pt);
void pmap_shootdown(struct pmap* pt);

#include <bits/pmap.h>

static inline size_t pmap_high_shift() {
    return arch_pmap_level_bits() * arch_pmap_num_levels() + arch_pmap_page_bits();
}

static inline void pmap_set(struct pmap* pt) {
    arch_pmap_set(pt);
}

static inline void pte_clear(pte_t* pte) {
    arch_pte_clear(pte);
}

static inline void pte_write(pte_t* pte, pte_t val) {
    arch_pte_write(pte, val);
}

static inline void pte_flush(uintptr_t vaddr) {
    arch_pte_flush(vaddr);
}

static inline void pte_flush_all() {
    arch_pte_flush_all();
}

#if CONFIG_HAVE_TLB_SHOOTDOWN_IPI
void pmap_shootdown_handle();

static inline void pte_shootdown_kick(struct percpu* cpu) {
    arch_pte_shootdown_kick(cpu);
}
#endif

static inline pte_t pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache) {
    return arch_pte_build(addr, flags, cache);
}

static inline pte_t pte_protect(pte_t old, enum pte_flags flags) {
    return arch_pte_protect(old, flags);
}

static inline pte_t pte_upgrade(pte_t old, enum pte_flags flags) {
    return arch_pte_upgrade(old, flags);
}

static inline bool pte_is_present(pte_t* pte) {
    return arch_pte_is_present(pte);
}

static inline phys_t pte_address(pte_t* pte) {
    return arch_pte_address(pte);
}
