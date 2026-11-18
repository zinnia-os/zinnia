// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/defs.h>
#include <arch/x86_64/pmap.h>
#include <boot/loader/loader.h>
#include <boot/loader/x86_64/loader_arch.h>

static bool next_table(uint64_t* table, size_t index, uint64_t** out) {
    if (!(table[index] & ARCH_FLAG_PRESENT)) {
        uint64_t page;
        if (!loader_alloc(PHYS_RECLAIMABLE, PAGE_SIZE, &page))
            return false;
        table[index] = page | ARCH_FLAG_PRESENT | ARCH_FLAG_READ_WRITE;
    }
    *out = loader_ptr(table[index] & ARCH_PTE_ADDR_MASK);
    return true;
}

bool arch_check_cpu() {
    uint64_t cr4;
    asm volatile("mov %0, cr4" : "=r"(cr4));
    if (cr4 & CR4_LA57)
        return false;
    return true;
}

bool arch_paging_map(uint64_t root, uint64_t virt, uint64_t phys, uint64_t size, uint64_t page_size) {
    const int leaf_level = page_size == ARCH_HUGE_PAGE_SIZE ? 1 : 0;

    for (uint64_t off = 0; off < size; off += page_size) {
        const uint64_t va = virt + off;
        uint64_t* table = loader_ptr(root);
        for (int level = 3; level > leaf_level; level--) {
            if (!next_table(table, (va >> (12 + 9 * level)) & 0x1ff, &table))
                return false;
        }

        uint64_t entry = (phys + off) | ARCH_FLAG_PRESENT | ARCH_FLAG_READ_WRITE;
        if (leaf_level == 1)
            entry |= ARCH_FLAG_SIZE;
        table[(va >> (12 + 9 * leaf_level)) & 0x1ff] = entry;
    }

    return true;
}
