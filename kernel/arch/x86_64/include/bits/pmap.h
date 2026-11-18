// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/x86_64/pmap.h>
#include <mem/pmap.h>
#include <stddef.h>
#include <stdint.h>

typedef uint64_t pte_t;

struct percpu;

void arch_pte_shootdown_kick(struct percpu* cpu);

static inline size_t arch_pmap_page_bits() {
    return 12;
}

static inline size_t arch_pmap_level_bits() {
    return 9;
}

static inline size_t arch_pmap_num_levels() {
    return 4;
}

static inline void arch_pte_clear(pte_t* pte) {
    *pte = 0;
}

static inline void arch_pte_write(pte_t* pte, pte_t val) {
    *pte = val;
}

static inline void arch_pte_flush(uintptr_t vaddr) {
    asm volatile("invlpg [%0]" ::"r"(vaddr) : "memory");
}

static inline void arch_pte_flush_all() {
    uint64_t cr3;
    asm volatile("mov %0, cr3" : "=r"(cr3));
    asm volatile("mov cr3, %0" ::"r"(cr3) : "memory");
}

static inline pte_t arch_pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache) {
    pte_t result = ((pte_t)addr & ARCH_PTE_ADDR_MASK) | ARCH_FLAG_PRESENT;

    if (flags & PTE_USER)
        result |= ARCH_FLAG_USER_MODE;

    if (flags & PTE_DIR)
        result |= ARCH_FLAG_READ_WRITE;
    else {
        if (flags & PTE_WRITE)
            result |= ARCH_FLAG_READ_WRITE;
        if (!(flags & PTE_EXEC))
            result |= ARCH_FLAG_EXECUTE_DISABLE;

        switch (cache) {
        case CACHE_ANY:
        case CACHE_WRITE_BACK:
            break;
        case CACHE_WRITE_THROUGH:
            result |= ARCH_FLAG_WRITE_THROUGH;
            break;
        case CACHE_MMIO:
            result |= ARCH_FLAG_CACHE_DISABLE | ARCH_FLAG_WRITE_THROUGH;
            break;
        case CACHE_WRITE_COMBINE:
            result |= ARCH_FLAG_ATTRIBUTE_TABLE | ARCH_FLAG_WRITE_THROUGH;
            break;
        }
    }
    return result;
}

static inline pte_t arch_pte_protect(pte_t old, enum pte_flags flags) {
    pte_t result = old & ~(ARCH_FLAG_READ_WRITE | ARCH_FLAG_USER_MODE | ARCH_FLAG_EXECUTE_DISABLE);
    if (flags & PTE_USER)
        result |= ARCH_FLAG_USER_MODE;
    if (flags & PTE_WRITE)
        result |= ARCH_FLAG_READ_WRITE;
    if (!(flags & PTE_EXEC))
        result |= ARCH_FLAG_EXECUTE_DISABLE;
    return result;
}

static inline pte_t arch_pte_upgrade(pte_t old, enum pte_flags flags) {
    return old | arch_pte_build(0, flags, CACHE_ANY);
}

static inline bool arch_pte_is_present(pte_t* pte) {
    return *pte & ARCH_FLAG_PRESENT;
}

static inline phys_t arch_pte_address(pte_t* pte) {
    return (phys_t)(*pte & ARCH_PTE_ADDR_MASK);
}

static inline void arch_pmap_set(struct pmap* pt) {
    asm volatile("mov cr3, %0" ::"r"(pt->root) : "memory");
}
