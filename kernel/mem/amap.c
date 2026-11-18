// SPDX-License-Identifier: BSD-3-Clause

#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/amap.h>
#include <mem/layout.h>
#include <mem/page.h>
#include <stdcountof.h>
#include <string.h>

errno_t anon_new(struct page* src, struct vm_anon** out) {
    struct vm_anon* anon = mem_alloc(sizeof(struct vm_anon), 0);
    if (!anon)
        return ENOMEM;

    struct page* page;
    errno_t status = page_alloc(src ? ALLOC_NOZERO : 0, &page);
    if (status) {
        mem_free(anon, sizeof(*anon));
        return status;
    }

    if (src)
        memcpy(HHDM_PTR(page_addr(page)), HHDM_PTR(page_addr(src)), page_size());

    atomic_fetch_or(&page->flags, PAGE_ANON, ATOMIC_RELAXED);
    page->obj.owner = anon;
    atomic_store(&anon->rc, 1, ATOMIC_RELAXED);
    anon->page = page;
    *out = anon;
    return 0;
}

void anon_ref(struct vm_anon* anon) {
    atomic_fetch_add(&anon->rc, 1, ATOMIC_RELAXED);
}

void anon_unref(struct vm_anon* anon) {
    if (atomic_fetch_sub(&anon->rc, 1, ATOMIC_RELEASE) != 1)
        return;
    atomic_fence(ATOMIC_ACQUIRE);
    page_unhold(anon->page);
    mem_free(anon, sizeof(*anon));
}

errno_t amap_new(struct amap** out) {
    struct amap* amap = mem_alloc(sizeof(struct amap), 0);
    if (!amap)
        return ENOMEM;
    refcount_init(&amap->rc);
    mutex_init(&amap->lock);
    radix_init(&amap->anons);
    *out = amap;
    return 0;
}

void amap_ref(struct amap* amap) {
    refcount_inc(&amap->rc);
}

void amap_unref(struct amap* amap) {
    if (!refcount_dec(&amap->rc))
        return;

    struct vm_anon* batch[32];
    uint64_t slots[32];
    size_t found;
    while ((found = radix_gang_lookup(&amap->anons, 0, countof(batch), (void**)batch, slots)) != 0) {
        for (size_t i = 0; i < found; i++) {
            radix_remove(&amap->anons, slots[i]);
            anon_unref(batch[i]);
        }
    }
    radix_destroy(&amap->anons);
    mem_free(amap, sizeof(*amap));
}

bool amap_is_shared(struct amap* amap) {
    return atomic_load(&amap->rc.count, ATOMIC_ACQUIRE) != 1;
}

errno_t amap_copy(struct amap* src, size_t offset, size_t num_slots, struct amap** out) {
    struct amap* copy;
    errno_t status = amap_new(&copy);
    if (status)
        return status;

    struct vm_anon* batch[32];
    uint64_t slots[32];
    uint64_t next = offset;

    mutex_lock(&src->lock);
    while (status == 0) {
        const size_t found = radix_gang_lookup(&src->anons, next, countof(batch), (void**)batch, slots);
        if (found == 0)
            break;
        for (size_t i = 0; i < found && status == 0; i++) {
            if (slots[i] >= offset + num_slots)
                goto done;
            status = radix_insert(&copy->anons, slots[i] - offset, batch[i]);
            if (!status)
                anon_ref(batch[i]);
        }
        next = slots[found - 1] + 1;
    }
done:
    mutex_unlock(&src->lock);

    if (status) {
        amap_unref(copy);
        return status;
    }
    *out = copy;
    return 0;
}
