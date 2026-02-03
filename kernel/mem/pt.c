#include <kernel/assert.h>
#include <kernel/mem.h>
#include <kernel/print.h>
#include <kernel/spin.h>

menix_status_t mem_pt_new_kernel(struct page_table* pt, enum alloc_flags flags) {
    flags &= ~ALLOC_NOZERO;

    phys_t addr;
    menix_status_t status = mem_phys_alloc(1, flags, &addr);
    if (status)
        return status;

    pt->root = addr;
    return 0;
}

menix_status_t mem_pt_new_user(struct page_table* pt, enum alloc_flags flags) {
    // TODO
    return 0;
}

// Gets a reference to the PTE at the given virtual address.
// If `check_only` is set, only checks if the PTE exists,
// and doesn't allocate new levels if they don't already exist.
// If it can't allocate a page if it has to, returns `nullptr`.
static menix_status_t get_pte(struct page_table* pt, virt_t vaddr, bool is_user, bool check_only, pte_t** ret) {
    pte_t* current_head = HHDM_PTR(pt->root);
    size_t index = 0;

    for (int8_t level = mem_num_levels() - 1; level >= 0; level--) {
        const size_t addr_mask = (1 << mem_level_bits()) - 1;
        const size_t addr_shift = mem_page_bits() + (mem_level_bits() * level);
        const enum pte_flags level_flags = PTE_DIR | (is_user ? PTE_USER : 0);

        index = (vaddr >> addr_shift) & addr_mask;

        // The last level is used to access the actual PTE, so break the loop then.
        // We still need to update the index beforehand, that's why we can't just end early.
        if (level == 0)
            break;

        pte_t* pte = &current_head[index];

        if (mem_pte_is_present(pte)) {
            // Get the next level.
            *pte = mem_pte_build(mem_pte_address(pte), level_flags, CACHE_NONE);
            current_head = HHDM_PTR(mem_pte_address(pte));
        } else {
            // If the current level isn't present, we can skip the rest.
            if (check_only)
                return MENIX_ERR_NO_MEMORY;

            phys_t addr;
            menix_status_t alloc_status = mem_phys_alloc(1, 0, &addr);
            if (alloc_status)
                return alloc_status;

            *pte = mem_pte_build(addr, level_flags, CACHE_NONE);
            current_head = HHDM_PTR(addr);
        }
    }

    *ret = &current_head[index];
    return 0;
}

menix_status_t mem_pt_map(
    struct page_table* pt,
    virt_t vaddr,
    phys_t paddr,
    enum pte_flags flags,
    enum cache_mode cache
) {
    spin_lock(&pt->lock);

    pte_t* pte;
    menix_status_t status = get_pte(pt, vaddr, flags & PTE_USER, false, &pte);
    if (status) {
        goto fail;
    }

    *pte = mem_pte_build(paddr, flags, cache);

fail:
    spin_unlock(&pt->lock);
    return status;
}
