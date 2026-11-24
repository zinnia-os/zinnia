// SPDX-License-Identifier: BSD-3-Clause

#include <lib/epoch.h>
#include <lib/errno.h>
#include <lib/print.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <stdcountof.h>
#include <string.h>

constexpr size_t RA_MIN_PAGES = 8;
constexpr size_t RA_MAX_PAGES = 64;

void vmo_init(struct vmo* vmo, const struct vmo_pager_ops* pager) {
    refcount_init(&vmo->rc);
    vmo->pager = pager;
    mutex_init(&vmo->lock);
    radix_init(&vmo->pages);
    tailq_init(&vmo->mappings);
    vmo->ra_next = UINT64_MAX;
    vmo->ra_window = RA_MIN_PAGES;
    vmo->cache = CACHE_ANY;
}

void vmo_ref(struct vmo* obj) {
    refcount_inc(&obj->rc);
}

static void release_pages(struct vmo* vmo) {
    struct page* batch[32];
    uint64_t indices[32];
    size_t found;
    while ((found = radix_gang_lookup(&vmo->pages, 0, countof(batch), (void**)batch, indices)) != 0) {
        for (size_t i = 0; i < found; i++) {
            radix_remove(&vmo->pages, indices[i]);
            page_unhold(batch[i]);
        }
    }
    radix_destroy(&vmo->pages);
}

void vmo_unref(struct vmo* obj) {
    if (!refcount_dec(&obj->rc))
        return;

    if (obj->pager->put_pages) {
        errno_t status = vmo_sync(obj, 0, UINT64_MAX);
        if (status)
            log_warn("vmo: Dropping dirty pages after failed writeback (%i)\n", status);
    }
    release_pages(obj);
    obj->pager->destroy(obj);
}

static void mark_dirty(struct vmo* vmo, struct page* page, uint64_t index) {
    if (!vmo->pager->put_pages)
        return;
    atomic_fetch_or(&page->flags, PAGE_DIRTY, ATOMIC_RELAXED);
    radix_tag_set(&vmo->pages, index, RADIX_TAG_DIRTY);
}

static void wait_busy(struct vmo* vmo, struct page* page) {
    page_hold(page);
    mutex_unlock(&vmo->lock);
    page_wait(page);
    page_unhold(page);
    mutex_lock(&vmo->lock);
}

static size_t cluster_size(struct vmo* vmo, uint64_t index) {
    const size_t max = MIN(vmo->pager->max_cluster, RA_MAX_PAGES);
    if (!vmo->pager->get_pages || max <= 1)
        return 1;

    vmo->ra_window = index == vmo->ra_next ? MIN(vmo->ra_window * 2, max) : MIN(RA_MIN_PAGES, max);
    size_t count = vmo->ra_window;
    if (index > UINT64_MAX - count)
        count = UINT64_MAX - index;
    vmo->ra_next = index + count;

    for (size_t i = 1; i < count; i++) {
        if (radix_lookup(&vmo->pages, index + i))
            return i;
    }
    return MAX(count, 1);
}

static errno_t fill(struct vmo* vmo, uint64_t index, enum vmo_access access, struct page** out) {
    struct page* pages[RA_MAX_PAGES];
    const bool read = vmo->pager->get_pages && access != VMO_OVERWRITE;
    size_t count = read ? cluster_size(vmo, index) : 1;
    const enum alloc_flags flags = read ? ALLOC_NOZERO : 0;

    for (size_t i = 0; i < count; i++) {
        struct page* page;
        errno_t status = page_alloc(flags, &page);
        if (!status) {
            page->obj.owner = vmo;
            page->obj.index = index + i;
            atomic_store(&page->flags, PAGE_BUSY, ATOMIC_RELAXED);
            status = radix_insert(&vmo->pages, index + i, page);
            if (status)
                page_unhold(page);
        }
        if (status) {
            if (i == 0) {
                mutex_unlock(&vmo->lock);
                return status;
            }
            count = i;
            break;
        }
        pages[i] = page;
    }

    page_hold(pages[0]);
    mutex_unlock(&vmo->lock);

    size_t valid = count;
    errno_t status = 0;
    if (read)
        status = vmo->pager->get_pages(vmo, index, pages, count, &valid);

    mutex_lock(&vmo->lock);
    for (size_t i = 0; i < count; i++) {
        if (status || i >= valid) {
            radix_remove(&vmo->pages, index + i);
            page_unbusy(pages[i]);
            page_unhold(pages[i]);
            continue;
        }
        if (i == 0 && access == VMO_WRITE)
            mark_dirty(vmo, pages[i], index);
        page_unbusy(pages[i]);
    }
    mutex_unlock(&vmo->lock);

    if (status || valid == 0) {
        page_unhold(pages[0]);
        return status ? status : ENXIO;
    }

    *out = pages[0];
    return 0;
}

static struct page* lookup_fast(struct vmo* vmo, uint64_t index) {
    epoch_enter();
    struct page* page = radix_lookup(&vmo->pages, index);
    if (page && !page_try_hold(page))
        page = nullptr;
    const bool valid =
        page && radix_lookup(&vmo->pages, index) == page && !(atomic_load(&page->flags, ATOMIC_ACQUIRE) & PAGE_BUSY);
    epoch_exit();

    if (page && !valid) {
        page_unhold(page);
        return nullptr;
    }
    return page;
}

errno_t vmo_get_page(struct vmo* vmo, uint64_t index, enum vmo_access access, struct page** out) {
    if (vmo->pager->lookup)
        return ENOTSUP;

    if (access != VMO_WRITE) {
        struct page* page = lookup_fast(vmo, index);
        if (page) {
            *out = page;
            return 0;
        }
    }

    mutex_lock(&vmo->lock);
    while (true) {
        struct page* page = radix_lookup(&vmo->pages, index);
        if (!page)
            return fill(vmo, index, access, out);

        if (atomic_load(&page->flags, ATOMIC_ACQUIRE) & PAGE_BUSY) {
            wait_busy(vmo, page);
            continue;
        }

        page_hold(page);
        if (access == VMO_WRITE)
            mark_dirty(vmo, page, index);
        mutex_unlock(&vmo->lock);
        *out = page;
        return 0;
    }
}

void vmo_put_page(struct page* page) {
    page_unhold(page);
}

static errno_t copy_data(
    struct vmo* obj,
    uint64_t offset,
    size_t len,
    enum vmo_access access,
    void* buf,
    iovec_iter_t* iter,
    size_t* out
) {
    const size_t pgsz = page_size();
    size_t progress = 0;
    errno_t status = 0;

    while (progress < len) {
        const size_t misalign = (offset + progress) % pgsz;
        const uint64_t index = (offset + progress) / pgsz;
        const size_t chunk = MIN(pgsz - misalign, len - progress);

        struct page* page = nullptr;
        phys_t phys;
        if (obj->pager->lookup) {
            status = obj->pager->lookup(obj, index, &phys);
        } else {
            status = vmo_get_page(obj, index, access, &page);
            if (!status)
                phys = page_addr(page);
        }
        if (status)
            break;

        uint8_t* ptr = (uint8_t*)HHDM_PTR(phys) + misalign;
        size_t done = chunk;
        if (iter && access == VMO_WRITE)
            status = iovec_iter_copy_from(iter, ptr, chunk, &done);
        else if (iter)
            status = iovec_iter_copy_to(iter, ptr, chunk, &done);
        else if (access == VMO_WRITE)
            memcpy(ptr, (uint8_t*)buf + progress, chunk);
        else
            memcpy((uint8_t*)buf + progress, ptr, chunk);

        if (page)
            vmo_put_page(page);
        progress += done;
        if (status || done < chunk)
            break;
    }

    if (out)
        *out = progress;
    return status;
}

errno_t vmo_read(struct vmo* obj, uint64_t offset, void* buf, size_t len, size_t* actual_read) {
    return copy_data(obj, offset, len, VMO_READ, buf, nullptr, actual_read);
}

errno_t vmo_write(struct vmo* obj, uint64_t offset, const void* buf, size_t len, size_t* actual_written) {
    return copy_data(obj, offset, len, VMO_WRITE, (void*)buf, nullptr, actual_written);
}

errno_t vmo_read_iter(struct vmo* obj, uint64_t offset, iovec_iter_t* iter, size_t len, size_t* actual_read) {
    return copy_data(obj, offset, len, VMO_READ, nullptr, iter, actual_read);
}

errno_t vmo_write_iter(struct vmo* obj, uint64_t offset, iovec_iter_t* iter, size_t len, size_t* actual_written) {
    return copy_data(obj, offset, len, VMO_WRITE, nullptr, iter, actual_written);
}

errno_t vmo_copy(
    struct vmo* target,
    uint64_t target_offset,
    struct vmo* src,
    uint64_t src_offset,
    size_t len,
    size_t* actual_copied
) {
    const size_t pgsz = page_size();
    size_t progress = 0;
    errno_t status = 0;

    while (progress < len) {
        const size_t target_misalign = (progress + target_offset) % pgsz;
        const size_t src_misalign = (progress + src_offset) % pgsz;
        const size_t copy_size = MIN(MIN(pgsz - target_misalign, pgsz - src_misalign), len - progress);

        struct page* tpage;
        status = vmo_get_page(target, (progress + target_offset) / pgsz, VMO_WRITE, &tpage);
        if (status)
            break;

        struct page* spage;
        status = vmo_get_page(src, (progress + src_offset) / pgsz, VMO_READ, &spage);
        if (status) {
            vmo_put_page(tpage);
            break;
        }

        memcpy(
            (uint8_t*)HHDM_PTR(page_addr(tpage)) + target_misalign,
            (uint8_t*)HHDM_PTR(page_addr(spage)) + src_misalign,
            copy_size
        );
        vmo_put_page(spage);
        vmo_put_page(tpage);
        progress += copy_size;
    }

    if (actual_copied)
        *actual_copied = progress;
    return status;
}

errno_t vmo_sync(struct vmo* vmo, uint64_t first, uint64_t end) {
    if (!vmo->pager->put_pages)
        return 0;

    struct page* run[RA_MAX_PAGES];
    const size_t max = MAX(MIN(vmo->pager->max_cluster, RA_MAX_PAGES), 1);
    errno_t result = 0;

    mutex_lock(&vmo->lock);
    while (first < end) {
        struct page* found;
        uint64_t index;
        if (!radix_gang_lookup_tag(&vmo->pages, first, RADIX_TAG_DIRTY, 1, (void**)&found, &index) || index >= end)
            break;
        if (!page_try_busy(found)) {
            wait_busy(vmo, found);
            continue;
        }

        size_t count = 1;
        run[0] = found;
        while (count < max && index + count < end && index + count != 0) {
            struct page* next = radix_lookup(&vmo->pages, index + count);
            if (!next || !radix_tag_get(&vmo->pages, index + count, RADIX_TAG_DIRTY) || !page_try_busy(next))
                break;
            run[count++] = next;
        }

        for (size_t i = 0; i < count; i++) {
            radix_tag_clear(&vmo->pages, index + i, RADIX_TAG_DIRTY);
            radix_tag_set(&vmo->pages, index + i, RADIX_TAG_WRITEBACK);
            atomic_fetch_and(&run[i]->flags, ~PAGE_DIRTY, ATOMIC_RELAXED);
            page_hold(run[i]);
            vma_object_remove_page(vmo, index + i, page_addr(run[i]), true);
        }
        mutex_unlock(&vmo->lock);

        errno_t status = vmo->pager->put_pages(vmo, index, run, count);

        mutex_lock(&vmo->lock);
        for (size_t i = 0; i < count; i++) {
            radix_tag_clear(&vmo->pages, index + i, RADIX_TAG_WRITEBACK);
            if (status)
                mark_dirty(vmo, run[i], index + i);
            page_unbusy(run[i]);
            page_unhold(run[i]);
        }
        if (status && !result)
            result = status;
        first = index + count;
        if (first == 0)
            break;
    }
    mutex_unlock(&vmo->lock);
    return result;
}

void vmo_truncate(struct vmo* vmo, uint64_t length) {
    const size_t pgsz = page_size();
    uint64_t first = length / pgsz + (length % pgsz != 0);

    mutex_lock(&vmo->lock);
    while (true) {
        struct page* page;
        uint64_t index;
        if (!radix_gang_lookup(&vmo->pages, first, 1, (void**)&page, &index))
            break;
        if (!page_try_busy(page)) {
            wait_busy(vmo, page);
            continue;
        }
        vma_object_remove_page(vmo, index, page_addr(page), false);
        radix_remove(&vmo->pages, index);
        page_unbusy(page);
        page_unhold(page);
        first = index + 1;
    }

    if (length % pgsz != 0) {
        while (true) {
            struct page* tail = radix_lookup(&vmo->pages, length / pgsz);
            if (!tail)
                break;
            if (atomic_load(&tail->flags, ATOMIC_ACQUIRE) & PAGE_BUSY) {
                wait_busy(vmo, tail);
                continue;
            }
            memset((uint8_t*)HHDM_PTR(page_addr(tail)) + length % pgsz, 0, pgsz - length % pgsz);
            mark_dirty(vmo, tail, length / pgsz);
            break;
        }
    }
    mutex_unlock(&vmo->lock);
}

struct anon_vmo {
    struct vmo vmo;
};

static void anon_destroy(struct vmo* vmo) {
    mem_free(containerof(vmo, struct anon_vmo, vmo), sizeof(struct anon_vmo));
}

static const struct vmo_pager_ops anon_pager = {
    .destroy = anon_destroy,
};

errno_t vmo_new_anon(struct vmo** out) {
    struct anon_vmo* anon = mem_alloc(sizeof(struct anon_vmo), 0);
    if (!anon)
        return ENOMEM;
    vmo_init(&anon->vmo, &anon_pager);
    *out = &anon->vmo;
    return 0;
}
