// SPDX-License-Identifier: BSD-3-Clause

#include <lib/errno.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/stack.h>
#include <mem/vma.h>

errno_t stack_init(struct stack* ks, size_t size) {
    if (!ks || size == 0)
        return EINVAL;

    const size_t pgsz = page_size();
    size = ROUND_UP(size, pgsz);

    uintptr_t va = vm_alloc(size + pgsz);
    if (!va)
        return ENOMEM;

    phys_t phys;
    errno_t status = mem_phys_alloc(size / pgsz, 0, &phys);
    if (status)
        goto err_vm;

    const uintptr_t base = va + pgsz;
    size_t mapped = 0;
    for (; mapped < size; mapped += pgsz) {
        status = pmap_map(&kernel_vma.pmap, base + mapped, phys + mapped, PTE_READ | PTE_WRITE, CACHE_WRITE_BACK);
        if (status)
            goto err_map;
    }

    ks->base = base;
    ks->phys = phys;
    ks->size = size;
    return 0;

err_map:
    for (size_t i = 0; i < mapped; i += pgsz)
        pmap_unmap(&kernel_vma.pmap, base + i);
    mem_phys_free(phys, size / pgsz);
err_vm:
    vm_free(va, size + pgsz);
    return status;
}

void stack_destroy(struct stack* ks) {
    if (!ks || !ks->base)
        return;

    const size_t pgsz = page_size();
    for (size_t i = 0; i < ks->size; i += pgsz)
        pmap_unmap(&kernel_vma.pmap, ks->base + i);
    pmap_shootdown(&kernel_vma.pmap);

    mem_phys_free(ks->phys, ks->size / pgsz);
    vm_free(ks->base - pgsz, ks->size + pgsz);

    *ks = (struct stack){0};
}
