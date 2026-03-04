#include <zinnia/status.h>
#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/vmspace.h>

// Try to create a new, empty virtual address space.
zn_status_t vmspace_new(struct vmspace** out) {
    DEBUG_ASSERT(out != nullptr, "Missing out ptr");
    zn_status_t res;
    if (!out)
        return ZN_ERR_INVALID;

    struct vmspace* vmspace = mem_alloc(sizeof(struct vmspace), 0);
    if (!vmspace)
        return ZN_ERR_NO_MEMORY;

    res = pmap_new_user(&vmspace->pmap, 0);
    if (res)
        goto err1;

    *out = vmspace;
    return ZN_OK;

err1:
    mem_free(vmspace);
err0:
    return res;
}

// Delete a virtual address space.
void vmspace_delete(struct vmspace* vm) {}

/*
// Create a new mapping.
zn_status_t vmspace_map(
    struct vmspace* vm,
    uintptr_t vaddr,
    size_t size,
    enum zn_vm_flags flags,
    struct rc* vmo,
    uintptr_t offset
) {}
*/

// Remove any mappings in the given address range.
// May fail if there is no memory and a mapping needs to be split.
zn_status_t vmspace_unmap(struct vmspace* vm, uintptr_t vaddr, size_t size);

// Change the protection of an existing mapping.
// May fail if there is no memory and a mapping needs to be split.
zn_status_t vmspace_protect(struct vmspace* vm, uintptr_t vaddr, size_t size, enum zn_vm_flags flags);

// Try to handle a page fault.
bool vmspace_page_fault(struct vmspace* vm, uintptr_t vaddr, enum zn_vm_flags access_type);

zn_status_t vmspace_map_vmo(
    struct vmspace* vas,
    struct vmo* vmo,
    uintptr_t addr,
    size_t len,
    enum zn_vm_flags flags,
    uintptr_t vmo_offset
) {
    const size_t page_size = arch_mem_page_size();

    if (addr % page_size != vmo_offset % page_size)
        return ZN_ERR_INVALID;

    size_t start_page = addr / page_size;
    const size_t addr_offset = addr % page_size;
    const size_t pages = ALIGN_UP(addr_offset + len, page_size) / page_size;
    size_t end_page = start_page + pages;

    if (!vas || !vmo)
        return ZN_ERR_INVALID;

    const size_t first_vmo_page = vmo_offset / page_size;

    for (size_t p = start_page; p < end_page; ++p) {
        const size_t page_index = p - start_page;
        struct page* page_obj;
        zn_status_t status = vmo->get_page(vmo, first_vmo_page + page_index, &page_obj);
        if (status != ZN_OK)
            return status;

        phys_t paddr = (phys_t)vm_page_addr(page_obj);

        enum pte_flags pf = 0;
        if (flags & ZN_VM_MAP_READ)
            pf |= PTE_READ;
        if (flags & ZN_VM_MAP_WRITE)
            pf |= PTE_WRITE;
        if (flags & ZN_VM_MAP_EXEC)
            pf |= PTE_EXEC;
        if (vas->pmap.is_user)
            pf |= PTE_USER;

        status = pmap_map(&vas->pmap, p * page_size, paddr, pf, CACHE_NONE);
        if (status != ZN_OK)
            return status;
    }

    return ZN_OK;
}
