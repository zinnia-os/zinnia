#include <zinnia/status.h>
#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/vas.h>

zn_status_t vas_new(struct vas** out) {
    if (!out)
        return ZN_ERR_INVALID;

    struct vas* result = mem_alloc(sizeof(struct vas), 0);
    if (!result)
        return ZN_ERR_NO_MEMORY;

    zn_status_t status = pt_new_user(&result->pt, 0);
    if (status)
        return status;

    *out = result;
    return ZN_OK;
}

zn_status_t vas_map_vmo(
    struct vas* vas,
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
        if (vas->pt.is_user)
            pf |= PTE_USER;

        status = pt_map(&vas->pt, p * page_size, paddr, pf, CACHE_NONE);
        if (status != ZN_OK)
            return status;
    }

    return ZN_OK;
}
