#include <kernel/alloc.h>
#include <kernel/pmap.h>
#include <kernel/spin.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>
#include <string.h>

errno_t pmap_new_kernel(struct pmap* pt, enum alloc_flags flags) {
    flags &= ~ALLOC_NOZERO;

    phys_t addr;
    errno_t status = mem_phys_alloc(1, flags, &addr);
    if (status)
        return status;

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
    errno_t status = mem_phys_alloc(1, 0, &user_l1);
    if (status)
        return status;

    void* user_l1_ptr = HHDM_PTR(user_l1);
    void* kernel_l1_ptr = HHDM_PTR(kernel_space.pmap.root);
    memcpy(user_l1_ptr, kernel_l1_ptr, arch_mem_page_size());

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

    for (int8_t level = arch_mem_num_levels() - 1; level >= 0; level--) {
        const size_t addr_mask = (1 << arch_mem_level_bits()) - 1;
        const size_t addr_shift = arch_mem_page_bits() + (arch_mem_level_bits() * level);
        const enum pte_flags level_flags = PTE_DIR | (is_user ? PTE_USER : 0);

        index = (vaddr >> addr_shift) & addr_mask;

        // The last level is used to access the actual PTE, so break the loop then.
        // We still need to update the index beforehand, that's why we can't just end early.
        if (level == 0)
            break;

        pte_t* pte = &current_head[index];

        if (pte_is_present(pte)) {
            // Get the next level.
            *pte = pte_build(pte_address(pte), level_flags, CACHE_NONE);
            current_head = HHDM_PTR(pte_address(pte));
        } else {
            // If the current level isn't present, we can skip the rest.
            if (check_only)
                return nullptr;

            phys_t addr;
            if (mem_phys_alloc(1, 0, &addr) != 0)
                return nullptr;

            *pte = pte_build(addr, level_flags, CACHE_NONE);
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

    *pte = pte_build(paddr, flags, cache);

fail:
    spin_unlock(&pt->lock);
    return status;
}
