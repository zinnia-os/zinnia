#include <zinnia/status.h>
#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/list.h>
#include <kernel/virt.h>
#include <kernel/vmo.h>
#include <string.h>

zn_status_t vmo_read(struct vmo* obj, uintptr_t offset, void* buf, size_t len, size_t* actual_read) {
    const size_t page_size = arch_mem_page_size();
    size_t progress = 0;
    zn_status_t status = ZN_OK;

    while (progress < len) {
        const size_t misalign = (progress + offset) % page_size;
        const size_t page_index = (progress + offset) / page_size;
        const size_t copy_size = MIN(page_size - misalign, len - progress);

        struct page* p;
        status = obj->get_page(obj, page_index, &p);
        if (status != ZN_OK)
            break;

        uintptr_t page_addr = vm_page_addr(p);

        memcpy(buf + progress, HHDM_PTR(page_addr) + misalign, copy_size);
        progress += copy_size;
    }

    if (actual_read)
        *actual_read = progress;
    return status;
}

zn_status_t vmo_write(struct vmo* obj, uintptr_t offset, const void* buf, size_t len, size_t* actual_written) {
    const size_t page_size = arch_mem_page_size();
    size_t progress = 0;
    zn_status_t status = ZN_OK;

    while (progress < len) {
        const size_t misalign = (progress + offset) % page_size;
        const size_t page_index = (progress + offset) / page_size;
        const size_t copy_size = MIN(page_size - misalign, len - progress);

        struct page* p;
        status = obj->get_page(obj, page_index, &p);
        if (status != ZN_OK)
            break;

        uintptr_t page_addr = vm_page_addr(p);

        memcpy(HHDM_PTR(page_addr) + misalign, buf + progress, copy_size);
        progress += copy_size;
    }

    if (actual_written)
        *actual_written = progress;
    return status;
}

zn_status_t vmo_copy(
    struct vmo* target,
    uintptr_t target_offset,
    struct vmo* src,
    uintptr_t src_offset,
    size_t len,
    size_t* actual_copied
) {
    const size_t page_size = arch_mem_page_size();
    size_t progress = 0;
    zn_status_t status = ZN_OK;

    while (progress < len) {
        const size_t target_misalign = (progress + target_offset) % page_size;
        const size_t src_misalign = (progress + src_offset) % page_size;
        const size_t target_page_index = (progress + target_offset) / page_size;
        const size_t src_page_index = (progress + src_offset) / page_size;

        const size_t copy_size = MIN(MIN(page_size - target_misalign, page_size - src_misalign), len - progress);

        struct page* tpage;
        status = target->get_page(target, target_page_index, &tpage);
        if (status != ZN_OK)
            break;

        struct page* spage;
        status = src->get_page(src, src_page_index, &spage);
        if (status != ZN_OK)
            break;

        uintptr_t taddr = vm_page_addr(tpage);
        uintptr_t saddr = vm_page_addr(spage);

        memcpy(HHDM_PTR(taddr) + target_misalign, HHDM_PTR(saddr) + src_misalign, copy_size);
        progress += copy_size;
    }

    if (actual_copied)
        *actual_copied = progress;
    return status;
}

static zn_status_t paged_get_page(struct vmo* vmo, uintptr_t offset_idx, struct page** out) {
    struct paged_vmo* paged = CONTAINER_OF(vmo, struct paged_vmo, object);

    // Try to find the requested page in the cache.
    struct page_list* iter;
    SLIST_FOREACH(iter, &paged->cache, next) {
        if (iter->offset != offset_idx)
            continue;

        // We found a cached page!
        *out = iter->value;
        return ZN_OK;
    }

    // Get page from out pager if we don't have one already.
    struct page* new_page;
    zn_status_t status = paged->source.get_page(&paged->source, offset_idx, &new_page);
    if (status)
        return status;

    struct page_list* entry = mem_alloc(sizeof(struct page_list), 0);
    if (!entry)
        return ZN_ERR_NO_MEMORY;

    entry->offset = offset_idx;
    entry->value = new_page;

    // Add page to the object cache.
    SLIST_INSERT_HEAD(&paged->cache, entry, next);

    *out = new_page;
    return ZN_OK;
}

zn_status_t vmo_new_paged(struct pager_ops pager, struct paged_vmo** out) {
    struct paged_vmo* result = mem_alloc(sizeof(struct paged_vmo), 0);
    if (!result)
        return ZN_ERR_NO_MEMORY;

    result->source = pager;
    result->object.get_page = paged_get_page;

    *out = result;
    return ZN_OK;
}

static zn_status_t phys_get_page(struct pager_ops* pager, uintptr_t offset, struct page** out) {
    phys_t addr;
    zn_status_t status = mem_phys_alloc(1, 0, &addr);
    if (status != ZN_OK)
        return status;
    size_t idx = addr / arch_mem_page_size();
    *out = &vm_pfndb[idx];
    return ZN_OK;
}

static zn_status_t phys_put_page(struct pager_ops* pager, uintptr_t offset, struct page* page) {
    // We do nothing here.
    return ZN_OK;
}

static const struct pager_ops phys_pager = {
    .get_page = phys_get_page,
    .put_page = phys_put_page,
};

zn_status_t vmo_new_phys(struct paged_vmo** out) {
    return vmo_new_paged(phys_pager, out);
}
