// SPDX-License-Identifier: BSD-3-Clause

#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/mmio.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>

errno_t mmio_map(phys_t base, size_t size, enum cache_mode cache, void** out) {
    const size_t pgsz = page_size();
    const phys_t start = ROUND_DOWN(base, pgsz);
    const size_t length = ROUND_UP(base + MAX(size, 1), pgsz) - start;

    const uintptr_t vaddr = vm_alloc(length);
    if (!vaddr)
        return ENOMEM;
    for (size_t off = 0; off < length; off += pgsz) {
        errno_t status = pmap_map(&kernel_vma.pmap, vaddr + off, start + off, PTE_READ | PTE_WRITE, cache);
        if (status) {
            for (size_t undo = 0; undo < off; undo += pgsz)
                pmap_unmap(&kernel_vma.pmap, vaddr + undo);
            vm_free(vaddr, length);
            return status;
        }
    }

    *out = (void*)(vaddr + (base - start));
    return 0;
}
