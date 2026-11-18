// SPDX-License-Identifier: BSD-3-Clause

#include <lib/errno.h>
#include <lib/spin.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <config.h>

// TODO: For now, we assume that we can do generic PTE lookups on every arch that has a MMU.
static_assert(CONFIG_HAVE_MMU_GENERIC);

errno_t pmap_new_kernel(struct pmap* pt, enum alloc_flags flags) {
    flags &= ~ALLOC_NOZERO;

    phys_t addr;
    errno_t status = mem_phys_alloc(1, flags, &addr);
    if (status)
        return status;

#if !CONFIG_HAVE_MMU_SPLIT_ROOT
    pte_t* root = HHDM_PTR(addr);
    const size_t num_entries = (size_t)1 << arch_pmap_level_bits();
    for (size_t i = num_entries / 2; i < num_entries; i++) {
        phys_t dir;
        status = mem_phys_alloc(1, flags, &dir);
        if (status)
            return status;
        pte_write(&root[i], pte_build(dir, PTE_DIR, CACHE_ANY));
    }
#endif

    struct pmap result = {
        .root = addr,
        .lock = (struct spinlock){0},
        .is_user = false,
    };

    *pt = result;
    return 0;
}

errno_t pmap_new_user(struct pmap* pt, enum alloc_flags flags) {
    phys_t user_l1;

    // If the kernel and userspace PTEs are on separate page tables,
    // allocate them individually. Otherwise, copy over the higher half PTEs
    // into the new user page table from the kernel VMA.
#if CONFIG_HAVE_MMU_SPLIT_ROOT
    errno_t status = mem_phys_alloc(1, flags & ~ALLOC_NOZERO, &user_l1);
    if (status)
        return status;
#else
    errno_t status = mem_phys_alloc(1, flags, &user_l1);
    if (status)
        return status;

    void* user_l1_ptr = HHDM_PTR(user_l1);
    void* kernel_l1_ptr = HHDM_PTR(kernel_vma.pmap.root);
    memcpy(user_l1_ptr, kernel_l1_ptr, page_size());
#endif

    struct pmap result = {
        .root = user_l1,
        .lock = (struct spinlock){0},
        .is_user = true,
    };

    *pt = result;
    return 0;
}

// Gets a reference to the PTE at the given virtual address.
// If `check_only` is set, only checks if the PTE exists,
// and doesn't allocate new levels if they don't already exist.
// If it can't allocate a page if it has to, returns `nullptr`.
static pte_t* get_pte(struct pmap* pt, uintptr_t vaddr, bool is_user, bool check_only) {
    pte_t* current_head = HHDM_PTR(pt->root);
    size_t index = 0;

    for (int8_t level = arch_pmap_num_levels() - 1; level >= 0; level--) {
        const size_t addr_mask = (1 << arch_pmap_level_bits()) - 1;
        const size_t addr_shift = arch_pmap_page_bits() + (arch_pmap_level_bits() * level);
        const enum pte_flags level_flags = PTE_DIR | (is_user ? PTE_USER : 0);

        index = (vaddr >> addr_shift) & addr_mask;

        // The last level is used to access the actual PTE, so break the loop then.
        // We still need to update the index beforehand, that's why we can't just end early.
        if (level == 0)
            break;

        pte_t* pte = &current_head[index];

        if (pte_is_present(pte)) {
            // Get the next level.
            const pte_t upgraded = pte_upgrade(*pte, level_flags);
            if (upgraded != *pte)
                pte_write(pte, upgraded);
            current_head = HHDM_PTR(pte_address(pte));
        } else {
            // If the current level isn't present, we can skip the rest.
            if (check_only)
                return nullptr;

            phys_t addr;
            if (mem_phys_alloc(1, 0, &addr) != 0)
                return nullptr;

            pte_write(pte, pte_build(addr, level_flags, CACHE_ANY));
            current_head = HHDM_PTR(addr);
        }
    }

    return &current_head[index];
}

errno_t pmap_map(struct pmap* pt, uintptr_t vaddr, phys_t paddr, enum pte_flags flags, enum cache_mode cache) {
    spin_lock(&pt->lock);

    errno_t status = 0;
    pte_t* pte = get_pte(pt, vaddr, flags & PTE_USER, false);
    if (pte == nullptr) {
        status = ENOMEM;
        goto fail;
    }

    if (pte_is_present(pte)) {
        pte_clear(pte);
        pte_flush(vaddr);
    }
    pte_write(pte, pte_build(paddr, flags, cache));

fail:
    spin_unlock(&pt->lock);
    return status;
}

errno_t pmap_unmap(struct pmap* pt, uintptr_t vaddr) {
    spin_lock(&pt->lock);

    errno_t status = 0;
    pte_t* pte = get_pte(pt, vaddr, false, true);
    if (pte == nullptr || !pte_is_present(pte)) {
        status = ENOENT;
        goto fail;
    }

    pte_clear(pte);
    pte_flush(vaddr);

fail:
    spin_unlock(&pt->lock);
    return status;
}

errno_t pmap_extract(struct pmap* pt, uintptr_t vaddr, phys_t* out) {
    spin_lock(&pt->lock);

    errno_t status = 0;
    pte_t* pte = get_pte(pt, vaddr, false, true);
    if (pte == nullptr || !pte_is_present(pte))
        status = ENOENT;
    else
        *out = pte_address(pte);

    spin_unlock(&pt->lock);
    return status;
}

errno_t pmap_protect(struct pmap* pt, uintptr_t vaddr, enum pte_flags flags) {
    spin_lock(&pt->lock);

    errno_t status = 0;
    pte_t* pte = get_pte(pt, vaddr, false, true);
    if (pte == nullptr || !pte_is_present(pte)) {
        status = ENOENT;
        goto fail;
    }

    pte_write(pte, pte_protect(*pte, flags));
    pte_flush(vaddr);

fail:
    spin_unlock(&pt->lock);
    return status;
}

static void free_table(phys_t table, int8_t level) {
    if (level > 0) {
        pte_t* entries = HHDM_PTR(table);
        const size_t num_entries = (size_t)1 << arch_pmap_level_bits();
        for (size_t i = 0; i < num_entries; i++) {
            if (pte_is_present(&entries[i]))
                free_table(pte_address(&entries[i]), level - 1);
        }
    }
    mem_phys_free(table, 1);
}

void pmap_destroy(struct pmap* pt) {
    if (!pt->is_user)
        return;

    pte_t* root = HHDM_PTR(pt->root);
    const size_t num_entries = (size_t)1 << arch_pmap_level_bits();
#if CONFIG_HAVE_MMU_SPLIT_ROOT
    const size_t user_entries = num_entries;
#else
    const size_t user_entries = num_entries / 2;
#endif
    for (size_t i = 0; i < user_entries; i++) {
        if (pte_is_present(&root[i]))
            free_table(pte_address(&root[i]), arch_pmap_num_levels() - 2);
    }
    mem_phys_free(pt->root, 1);
    pt->root = 0;
}
