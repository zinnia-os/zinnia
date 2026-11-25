// SPDX-License-Identifier: BSD-3-Clause

#include <arch/riscv64/defs.h>
#include <arch/riscv64/pmap.h>
#include <boot/loader/loader.h>
#include <boot/loader/riscv64/loader_arch.h>

static bool next_table(uint64_t* table, size_t index, uint64_t** out) {
    if (!(table[index] & ARCH_FLAG_VALID)) {
        uint64_t page;
        if (!loader_alloc(PHYS_RECLAIMABLE, PAGE_SIZE, &page))
            return false;
        table[index] = (page >> 12) << ARCH_PTE_PPN_SHIFT | ARCH_FLAG_VALID;
    }
    *out = loader_ptr(((table[index] & ARCH_PTE_PPN_MASK) >> ARCH_PTE_PPN_SHIFT) << 12);
    return true;
}

uint8_t loader_pmap_levels;

alignas(4096) static uint64_t probe_root[512];

static bool satp_levels_supported(uint8_t levels) {
    const unsigned shift = 12 + 9 * (levels - 1);
    for (size_t i = 0; i < 512; i++) {
        probe_root[i] = ((uint64_t)i << shift >> 12) << ARCH_PTE_PPN_SHIFT | ARCH_FLAG_VALID | ARCH_FLAG_READ |
            ARCH_FLAG_WRITE | ARCH_FLAG_EXECUTE | ARCH_FLAG_ACCESSED | ARCH_FLAG_DIRTY;
    }

    const uint64_t satp = arch_satp_build(levels, (uint64_t)probe_root);
    uint64_t sstatus, old, read;
    asm volatile(
        "csrrci %0, sstatus, %c[sie]\n"
        "csrr %1, satp\n"
        "csrw satp, %3\n"
        "sfence.vma\n"
        "csrr %2, satp\n"
        "csrw satp, %1\n"
        "sfence.vma\n"
        "csrw sstatus, %0\n"
        : "=&r"(sstatus), "=&r"(old), "=&r"(read)
        : "r"(satp), [sie] "i"(SSTATUS_SIE)
        : "memory"
    );
    return read == satp;
}

bool arch_check_cpu() {
    for (uint8_t levels = 4; levels >= 3; levels--) {
        if (satp_levels_supported(levels)) {
            loader_pmap_levels = levels;
            return true;
        }
    }
    return false;
}

bool arch_paging_map(uint64_t root, uint64_t virt, uint64_t phys, uint64_t size, uint64_t page_size) {
    const int leaf_level = page_size == ARCH_HUGE_PAGE_SIZE ? 1 : 0;
    const uint64_t leaf_flags =
        ARCH_FLAG_VALID | ARCH_FLAG_READ | ARCH_FLAG_WRITE | ARCH_FLAG_EXECUTE | ARCH_FLAG_ACCESSED | ARCH_FLAG_DIRTY;

    for (uint64_t off = 0; off < size; off += page_size) {
        const uint64_t va = virt + off;
        uint64_t* table = loader_ptr(root);
        for (int level = loader_pmap_levels - 1; level > leaf_level; level--) {
            if (!next_table(table, (va >> (12 + 9 * level)) & 0x1ff, &table))
                return false;
        }

        table[(va >> (12 + 9 * leaf_level)) & 0x1ff] = ((phys + off) >> 12) << ARCH_PTE_PPN_SHIFT | leaf_flags;
    }

    return true;
}
