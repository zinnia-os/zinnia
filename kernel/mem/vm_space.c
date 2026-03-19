#include <kernel/alloc.h>
#include <kernel/assert.h>
#include <kernel/utils.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>

// Try to create a new, empty virtual address space.
struct vm_space* vm_space_new() {
    struct vm_space* vm_space = mem_alloc(sizeof(struct vm_space), 0);
    if (!vm_space)
        return nullptr;

    vm_space->refcount = 1;

    errno_t res = pmap_new_user(&vm_space->pmap, 0);
    if (res)
        goto err1;

    return vm_space;

err1:
    mem_free(vm_space);
    return nullptr;
}

void vm_space_delete(struct vm_space* vm) {}

errno_t vm_space_unmap(struct vm_space* vm, uintptr_t vaddr, size_t size);

errno_t vm_space_map(
    struct vm_space* vas,
    struct vm_object* vm_object,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    uintptr_t vm_object_offset
) {
    const size_t page_size = arch_mem_page_size();

    if (addr % page_size != vm_object_offset % page_size)
        return EINVAL;

    size_t start_page = addr / page_size;
    const size_t addr_offset = addr % page_size;
    const size_t pages = ALIGN_UP(addr_offset + len, page_size) / page_size;
    size_t end_page = start_page + pages;

    if (!vas || !vm_object)
        return EINVAL;

    const size_t first_vm_object_page = vm_object_offset / page_size;

    for (size_t p = start_page; p < end_page; ++p) {
        const size_t page_index = p - start_page;
        struct page* page_obj;
        errno_t status = vm_object->get_page(vm_object, first_vm_object_page + page_index, &page_obj);
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
