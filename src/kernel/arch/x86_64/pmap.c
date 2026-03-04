#include <kernel/pmap.h>
#include <kernel/types.h>

enum : pte_t {
    ARCH_FLAG_PRESENT = 1 << 0,
    ARCH_FLAG_READ_WRITE = 1 << 1,
    ARCH_FLAG_USER_MODE = 1 << 2,
    ARCH_FLAG_WRITE_THROUGH = 1 << 3,
    ARCH_FLAG_CACHE_DISABLE = 1 << 4,
    ARCH_FLAG_ACCESSED = 1 << 5,
    ARCH_FLAG_DIRTY = 1 << 6,
    ARCH_FLAG_SIZE = 1 << 7,
    ARCH_FLAG_GLOBAL = 1 << 8,
    ARCH_FLAG_AVAILABLE = 1 << 9,
    ARCH_FLAG_ATTRIBUTE_TABLE = 1 << 10,
    ARCH_FLAG_EXECUTE_DISABLE = 1LU << 63,

    ARCH_PTE_ADDR_MASK = 0x000F'FFFF'FFFF'F000,
};

void pte_clear(pte_t* pte) {
    *pte = 0;
}

pte_t pte_build(phys_t addr, enum pte_flags flags, enum cache_mode cache) {
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
    }
    return result;
}

bool pte_is_present(pte_t* pte) {
    return *pte & ARCH_FLAG_PRESENT;
}

bool pte_is_dir(pte_t* pte) {
    return *pte & ARCH_FLAG_SIZE;
}

phys_t pte_address(pte_t* pte) {
    return (phys_t)(*pte & ARCH_PTE_ADDR_MASK);
}

void pmap_set(struct pmap* pt) {
    asm volatile("mov cr3, %0" ::"r"(pt->root) : "memory");
}
