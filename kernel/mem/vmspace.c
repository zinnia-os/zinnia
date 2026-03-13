#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/utils.h>
#include <kernel/vmspace.h>
#include <uapi/errno.h>

// Try to create a new, empty virtual address space.
struct vmspace* vmspace_new() {
    struct vmspace* vmspace = mem_alloc(sizeof(struct vmspace), 0);
    if (!vmspace)
        return nullptr;

    errno_t res = pmap_new_user(&vmspace->pmap, 0);
    if (res)
        goto err1;

    return vmspace;

err1:
    mem_free(vmspace);
    return nullptr;
}

void vmspace_delete(struct vmspace* vm) {}

errno_t vmspace_unmap(struct vmspace* vm, uintptr_t vaddr, size_t size);

errno_t vmspace_map(
    struct vmspace* vas,
    struct vmobject* vmobject,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    uintptr_t vmobject_offset
) {
    const size_t page_size = arch_mem_page_size();

    if (addr % page_size != vmobject_offset % page_size)
        return EINVAL;

    size_t start_page = addr / page_size;
    const size_t addr_offset = addr % page_size;
    const size_t pages = ALIGN_UP(addr_offset + len, page_size) / page_size;
    size_t end_page = start_page + pages;

    if (!vas || !vmobject)
        return EINVAL;

    const size_t first_vmobject_page = vmobject_offset / page_size;

    for (size_t p = start_page; p < end_page; ++p) {
        const size_t page_index = p - start_page;
        struct page* page_obj;
        errno_t status = vmobject->get_page(vmobject, first_vmobject_page + page_index, &page_obj);
        if (status != 0)
            return status;

        phys_t paddr = (phys_t)vm_page_addr(page_obj);

        enum pte_flags pf = 0;
        if (prot & PROT_READ)
            pf |= PTE_READ;
        if (prot & PROT_WRITE)
            pf |= PTE_WRITE;
        if (prot & PROT_EXEC)
            pf |= PTE_EXEC;
        if (vas->pmap.is_user)
            pf |= PTE_USER;

        status = pmap_map(&vas->pmap, p * page_size, paddr, pf, CACHE_NONE);
        if (status != 0)
            return status;
    }

    return 0;
}
