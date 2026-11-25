// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <arch/riscv64/pmap.h>
#include <mem/pmap.h>
#include <bits/mem.h>
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
    return riscv_pmap_levels;
}

static inline void arch_pte_clear(pte_t* pte) {
    *pte = 0;
}

static inline void arch_pte_write(pte_t* pte, pte_t val) {
    *pte = val;
}

static inline void arch_pte_flush(uintptr_t vaddr) {
    asm volatile("sfence.vma %0, zero" ::"r"(vaddr) : "memory");
}

static inline void arch_pte_flush_all() {
    asm volatile("sfence.vma" ::: "memory");
}

static inline pte_t arch_pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache) {
    pte_t result = (((pte_t)addr >> 12) << ARCH_PTE_PPN_SHIFT) | ARCH_FLAG_VALID;

    if (flags & PTE_DIR)
        return result;

    result |= ARCH_FLAG_READ | ARCH_FLAG_ACCESSED | ARCH_FLAG_DIRTY;
    if (flags & PTE_USER)
        result |= ARCH_FLAG_USER;
    if (flags & PTE_WRITE)
        result |= ARCH_FLAG_WRITE;
    if (flags & PTE_EXEC)
        result |= ARCH_FLAG_EXECUTE;
    return result;
}

static inline pte_t arch_pte_protect(pte_t old, enum pte_flags flags) {
    pte_t result = old & ~(ARCH_FLAG_WRITE | ARCH_FLAG_USER | ARCH_FLAG_EXECUTE);
    if (flags & PTE_USER)
        result |= ARCH_FLAG_USER;
    if (flags & PTE_WRITE)
        result |= ARCH_FLAG_WRITE;
    if (flags & PTE_EXEC)
        result |= ARCH_FLAG_EXECUTE;
    return result;
}

static inline pte_t arch_pte_upgrade(pte_t old, enum pte_flags flags) {
    return old | arch_pte_build(0, flags, CACHE_ANY);
}

static inline bool arch_pte_is_present(pte_t* pte) {
    return *pte & ARCH_FLAG_VALID;
}

static inline phys_t arch_pte_address(pte_t* pte) {
    return (phys_t)((*pte & ARCH_PTE_PPN_MASK) >> ARCH_PTE_PPN_SHIFT) << 12;
}

static inline void arch_pmap_set(struct pmap* pt) {
    asm volatile("csrw satp, %0\nsfence.vma" ::"r"(arch_satp_build(riscv_pmap_levels, pt->root)) : "memory");
}
