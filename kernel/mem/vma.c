// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/errno.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/amap.h>
#include <mem/layout.h>
#include <mem/mman.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stdckdint.h>
#include <stdcountof.h>
#include <string.h>

typedef tailq_head(struct vma_mapping) mapping_list_t;

constexpr uintptr_t VMA_MMAP_BASE = 0x1'0000'0000;

// Try to create a new, empty virtual address space.
errno_t vma_new(struct vma** out) {
    debug_assert(out != nullptr, "Missing out");

    errno_t res;
    if (!out)
        return EINVAL;

    struct vma* vmspace = mem_alloc(sizeof(struct vma), 0);
    if (!vmspace)
        return ENOMEM;

    res = pmap_new_user(&vmspace->pmap, 0);
    if (res)
        goto err1;

    tailq_init(&vmspace->mappings);
    rwlock_init(&vmspace->lock);
    mutex_init(&vmspace->map_lock);
    refcount_init(&vmspace->rc);

    *out = vmspace;
    return 0;

err1:
    mem_free(vmspace, sizeof(*vmspace));
    return res;
}

static void object_lock(struct vma_mapping* m) {
    if (m->object)
        mutex_lock(&m->object->lock);
}

static void object_unlock(struct vma_mapping* m) {
    if (m->object)
        mutex_unlock(&m->object->lock);
}

static void object_link(struct vma_mapping* m) {
    if (!m->object)
        return;
    mutex_lock(&m->object->lock);
    tailq_insert_tail(&m->object->mappings, m, object_link);
    mutex_unlock(&m->object->lock);
}

static void mapping_free(struct vma_mapping* m) {
    if (m->object) {
        mutex_lock(&m->object->lock);
        tailq_remove(&m->object->mappings, m, object_link);
        mutex_unlock(&m->object->lock);
        vmo_unref(m->object);
    }
    if (m->amap)
        amap_unref(m->amap);
    mem_free(m, sizeof(*m));
}

static void free_mappings(mapping_list_t* list) {
    struct vma_mapping* mapping;
    while ((mapping = tailq_first(list)) != nullptr) {
        tailq_remove(list, mapping, link);
        mapping_free(mapping);
    }
}

void vma_ref(struct vma* v) {
    refcount_inc(&v->rc);
}

void vma_unref(struct vma* v) {
    assert(v != &kernel_vma, "Tried to release the kernel address space");
    if (!refcount_dec(&v->rc))
        return;

    free_mappings((mapping_list_t*)&v->mappings);
    pmap_destroy(&v->pmap);
    mem_free(v, sizeof(*v));
}

static struct vma_mapping* mapping_alloc() {
    return mem_alloc(sizeof(struct vma_mapping), 0);
}

static void mapping_clone(
    struct vma_mapping* dst,
    const struct vma_mapping* src,
    struct vma* vma,
    size_t start,
    size_t end
) {
    const size_t delta = start - src->start_page;
    dst->start_page = start;
    dst->end_page = end;
    dst->offset_page = src->offset_page + delta;
    dst->object = src->object;
    dst->amap = src->amap;
    dst->amap_offset = src->amap_offset + delta;
    dst->vma = vma;
    dst->prot = src->prot;
    dst->flags = src->flags;
    if (dst->object)
        vmo_ref(dst->object);
    if (dst->amap)
        amap_ref(dst->amap);
}

static void unmap_pages(struct vma* v, size_t start, size_t end) {
    const size_t pgsz = page_size();
    for (size_t p = start; p < end; p++)
        pmap_unmap(&v->pmap, p * pgsz);
}

static void carve(struct vma* v, size_t start, size_t end, struct vma_mapping** spare, mapping_list_t* dead) {
    struct vma_mapping* m = tailq_first(&v->mappings);
    while (m) {
        struct vma_mapping* next = tailq_next(m, link);
        if (start < m->end_page && m->start_page < end) {
            unmap_pages(v, MAX(start, m->start_page), MIN(end, m->end_page));

            if (m->start_page >= start && m->end_page <= end) {
                tailq_remove(&v->mappings, m, link);
                tailq_insert_tail(dead, m, link);
            } else if (m->start_page < start && m->end_page > end) {
                struct vma_mapping* tail = *spare;
                assert(tail != nullptr, "Missing spare mapping");
                *spare = nullptr;
                mapping_clone(tail, m, v, end, m->end_page);
                object_lock(m);
                m->end_page = start;
                object_unlock(m);
                object_link(tail);
                tailq_insert_tail(&v->mappings, tail, link);
            } else if (m->start_page < start) {
                object_lock(m);
                m->end_page = start;
                object_unlock(m);
            } else {
                const size_t delta = end - m->start_page;
                object_lock(m);
                m->offset_page += delta;
                m->amap_offset += delta;
                m->start_page = end;
                object_unlock(m);
            }
        }
        m = next;
    }
}

static void split_at(struct vma* v, size_t page, struct vma_mapping** spare) {
    struct vma_mapping* m;
    for (m = tailq_first(&v->mappings); m; m = tailq_next(m, link)) {
        if (m->start_page < page && page < m->end_page)
            break;
    }
    if (!m)
        return;

    struct vma_mapping* tail = *spare;
    assert(tail != nullptr, "Missing spare mapping");
    *spare = nullptr;
    mapping_clone(tail, m, v, page, m->end_page);
    object_lock(m);
    m->end_page = page;
    object_unlock(m);
    object_link(tail);
    tailq_insert_tail(&v->mappings, tail, link);
}

static errno_t page_range(uintptr_t addr, size_t len, size_t* start, size_t* end) {
    const size_t pgsz = page_size();
    uintptr_t last;
    if (addr % pgsz != 0 || len == 0 || ckd_add(&last, addr, len - 1))
        return EINVAL;
    *start = addr / pgsz;
    *end = last / pgsz + 1;
    return 0;
}

static bool range_mapped(struct vma* v, size_t start, size_t end) {
    size_t page = start;
    while (page < end) {
        struct vma_mapping* m;
        for (m = tailq_first(&v->mappings); m; m = tailq_next(m, link)) {
            if (page >= m->start_page && page < m->end_page)
                break;
        }
        if (!m)
            return false;
        page = m->end_page;
    }
    return true;
}

bool vma_range_mapped(struct vma* v, uintptr_t vaddr, size_t size) {
    size_t start, end;
    if (page_range(vaddr, size, &start, &end))
        return false;

    rwlock_read_lock(&v->lock);
    bool mapped = range_mapped(v, start, end);
    rwlock_read_unlock(&v->lock);
    return mapped;
}

errno_t vma_find_free(struct vma* v, uintptr_t hint, size_t len, uintptr_t* out) {
    const size_t pgsz = page_size();
    if (len == 0)
        return EINVAL;

    uintptr_t base = MAX(hint, VMA_MMAP_BASE);
    if (ckd_add(&base, base, pgsz - 1))
        return ENOMEM;
    size_t candidate = base / pgsz;
    const size_t num_pages = len / pgsz + (len % pgsz != 0);

    rwlock_read_lock(&v->lock);
    while (true) {
        size_t candidate_end;
        if (ckd_add(&candidate_end, candidate, num_pages) || candidate_end > SIZE_MAX / pgsz ||
            !mem_addr_is_user(candidate_end * pgsz - 1)) {
            rwlock_read_unlock(&v->lock);
            return ENOMEM;
        }

        struct vma_mapping* m;
        for (m = tailq_first(&v->mappings); m; m = tailq_next(m, link)) {
            if (candidate < m->end_page && m->start_page < candidate_end)
                break;
        }
        if (!m)
            break;
        candidate = m->end_page;
    }
    rwlock_read_unlock(&v->lock);

    *out = candidate * pgsz;
    return 0;
}

errno_t vma_unmap(struct vma* v, uintptr_t vaddr, size_t size) {
    size_t start, end;
    errno_t status = page_range(vaddr, size, &start, &end);
    if (status)
        return status;

    struct vma_mapping* spare = mapping_alloc();
    if (!spare)
        return ENOMEM;

    mapping_list_t dead;
    tailq_init(&dead);

    rwlock_write_lock(&v->lock);
    carve(v, start, end, &spare, &dead);
    pmap_shootdown(&v->pmap);
    rwlock_write_unlock(&v->lock);

    free_mappings(&dead);
    mem_free(spare, sizeof(*spare));
    return 0;
}

static enum pte_flags prot_to_pte(struct vma* v, enum prot_flags prot) {
    enum pte_flags pf = 0;
    if (prot & PROT_READ)
        pf |= PTE_READ;
    if (prot & PROT_WRITE)
        pf |= PTE_WRITE;
    if (prot & PROT_EXEC)
        pf |= PTE_EXEC;
    if (v->pmap.is_user)
        pf |= PTE_USER;
    return pf;
}

static void reprotect_pages(struct vma* v, const struct vma_mapping* m) {
    const size_t pgsz = page_size();
    const bool none = !(m->prot & (PROT_READ | PROT_WRITE | PROT_EXEC));
    const enum pte_flags flags = prot_to_pte(v, m->prot & ~PROT_WRITE);
    for (size_t p = m->start_page; p < m->end_page; p++) {
        if (none)
            pmap_unmap(&v->pmap, p * pgsz);
        else
            pmap_protect(&v->pmap, p * pgsz, flags);
    }
}

errno_t vma_protect(struct vma* v, uintptr_t vaddr, size_t size, enum prot_flags flags) {
    size_t start, end;
    errno_t status = page_range(vaddr, size, &start, &end);
    if (status)
        return status;

    struct vma_mapping* spares[2] = {mapping_alloc(), mapping_alloc()};
    if (!spares[0] || !spares[1]) {
        mem_free(spares[0], sizeof(*spares[0]));
        mem_free(spares[1], sizeof(*spares[1]));
        return ENOMEM;
    }

    rwlock_write_lock(&v->lock);
    if (!range_mapped(v, start, end)) {
        rwlock_write_unlock(&v->lock);
        mem_free(spares[0], sizeof(*spares[0]));
        mem_free(spares[1], sizeof(*spares[1]));
        return ENOMEM;
    }
    split_at(v, start, &spares[0]);
    split_at(v, end, &spares[1]);
    for (struct vma_mapping* m = tailq_first(&v->mappings); m; m = tailq_next(m, link)) {
        if (m->start_page >= start && m->end_page <= end) {
            object_lock(m);
            m->prot = flags;
            object_unlock(m);
            reprotect_pages(v, m);
        }
    }
    pmap_shootdown(&v->pmap);
    rwlock_write_unlock(&v->lock);

    mem_free(spares[0], sizeof(*spares[0]));
    mem_free(spares[1], sizeof(*spares[1]));
    return 0;
}

void vma_object_remove_page(struct vmo* object, uint64_t index, phys_t phys, bool protect) {
    const size_t pgsz = page_size();
    for (struct vma_mapping* m = tailq_first(&object->mappings); m; m = tailq_next(m, object_link)) {
        if (index < m->offset_page || index - m->offset_page >= m->end_page - m->start_page)
            continue;

        struct pmap* pmap = &m->vma->pmap;
        const uintptr_t va = (m->start_page + (index - m->offset_page)) * pgsz;
        phys_t current;
        if (pmap_extract(pmap, va, &current) || current != phys)
            continue;

        if (protect)
            pmap_protect(pmap, va, prot_to_pte(m->vma, m->prot & ~PROT_WRITE));
        else
            pmap_unmap(pmap, va);
        pmap_shootdown(pmap);
    }
}

static struct vma_mapping* find_mapping(struct vma* v, size_t page) {
    for (struct vma_mapping* m = tailq_first(&v->mappings); m; m = tailq_next(m, link)) {
        if (page >= m->start_page && page < m->end_page)
            return m;
    }
    return nullptr;
}

enum fault_kind {
    FAULT_READ,
    FAULT_WRITE,
    FAULT_EXEC,
};

static bool can_access(enum prot_flags prot, enum fault_kind kind) {
    switch (kind) {
    case FAULT_WRITE:
        return prot & PROT_WRITE;
    case FAULT_EXEC:
        return prot & PROT_EXEC;
    default:
        return prot & (PROT_READ | PROT_WRITE);
    }
}

static errno_t enter(struct vma* v, size_t page, phys_t phys, enum pte_flags flags, enum cache_mode cache) {
    const uintptr_t va = page * page_size();
    phys_t old;
    const bool replace = pmap_extract(&v->pmap, va, &old) == 0;
    errno_t status = pmap_map(&v->pmap, va, phys, flags, cache);
    if (status)
        return status;
    pte_flush(va);
    if (replace)
        pmap_shootdown(&v->pmap);
    return 0;
}

static errno_t prepare_amap(struct vma* v, size_t page) {
    errno_t status = 0;
    rwlock_write_lock(&v->lock);
    struct vma_mapping* m = find_mapping(v, page);
    if (m && !(m->flags & VMA_SHARED)) {
        if (!m->amap) {
            status = amap_new(&m->amap);
            m->amap_offset = 0;
            if (!status)
                m->flags &= ~VMA_NEEDS_COPY;
        } else if (m->flags & VMA_NEEDS_COPY) {
            if (amap_is_shared(m->amap)) {
                struct amap* copy;
                status = amap_copy(m->amap, m->amap_offset, m->end_page - m->start_page, &copy);
                if (!status) {
                    amap_unref(m->amap);
                    m->amap = copy;
                    m->amap_offset = 0;
                }
            }
            if (!status)
                m->flags &= ~VMA_NEEDS_COPY;
        }
    }
    rwlock_write_unlock(&v->lock);
    return status;
}

static errno_t fault_anon(
    struct vma* v,
    struct vma_mapping* m,
    struct vm_anon* anon,
    size_t page,
    size_t slot,
    bool write,
    phys_t* out
) {
    bool writable = !(m->flags & VMA_NEEDS_COPY) && atomic_load(&anon->rc, ATOMIC_ACQUIRE) == 1;
    if (write && !writable) {
        struct vm_anon* copy;
        errno_t status = anon_new(anon->page, &copy);
        if (status)
            return status;
        radix_replace(&m->amap->anons, slot, copy);
        anon_unref(anon);
        anon = copy;
        writable = true;
    }

    *out = page_addr(anon->page);
    return enter(v, page, *out, prot_to_pte(v, writable ? m->prot : m->prot & ~PROT_WRITE), CACHE_ANY);
}

static errno_t fault_promote(
    struct vma* v,
    struct vma_mapping* m,
    size_t page,
    size_t slot,
    size_t index,
    phys_t* out
) {
    struct vmo* obj = m->object;
    struct page* src = nullptr;
    errno_t status = 0;
    if (obj && obj->pager->lookup) {
        phys_t phys;
        status = obj->pager->lookup(obj, index, &phys);
        if (!status)
            src = page_from_addr(phys);
    } else if (obj) {
        status = vmo_get_page(obj, index, VMO_READ, &src);
    }
    if (status)
        return status;

    struct vm_anon* anon;
    status = anon_new(src, &anon);
    if (src && !obj->pager->lookup)
        vmo_put_page(src);
    if (status)
        return status;

    status = radix_insert(&m->amap->anons, slot, anon);
    if (status) {
        anon_unref(anon);
        return status;
    }

    *out = page_addr(anon->page);
    return enter(v, page, *out, prot_to_pte(v, m->prot), CACHE_ANY);
}

static errno_t fault_object(struct vma* v, struct vma_mapping* m, size_t page, size_t index, bool write, phys_t* out) {
    struct vmo* obj = m->object;
    const bool shared = m->flags & VMA_SHARED;

    if (obj->pager->lookup) {
        errno_t status = obj->pager->lookup(obj, index, out);
        if (status)
            return status;
        return enter(v, page, *out, prot_to_pte(v, shared ? m->prot : m->prot & ~PROT_WRITE), obj->cache);
    }

    struct page* pg;
    errno_t status = vmo_get_page(obj, index, shared && write ? VMO_WRITE : VMO_READ, &pg);
    if (status)
        return status;

    const bool writable = shared && (write || !obj->pager->put_pages);
    mutex_lock(&obj->lock);
    if (radix_lookup(&obj->pages, index) == pg) {
        *out = page_addr(pg);
        status = enter(v, page, *out, prot_to_pte(v, writable ? m->prot : m->prot & ~PROT_WRITE), obj->cache);
    } else {
        status = EAGAIN;
    }
    mutex_unlock(&obj->lock);
    vmo_put_page(pg);
    return status;
}

static errno_t fault(struct vma* v, size_t page, enum fault_kind kind, bool force, phys_t* out) {
    const bool write = kind == FAULT_WRITE;

    while (true) {
        rwlock_read_lock(&v->lock);
        struct vma_mapping* m = find_mapping(v, page);
        if (!m) {
            rwlock_read_unlock(&v->lock);
            return EFAULT;
        }
        if (!force && !can_access(m->prot, kind)) {
            rwlock_read_unlock(&v->lock);
            return EACCES;
        }

        const bool shared = m->flags & VMA_SHARED;
        if (!shared && (write || !m->object) && (!m->amap || (m->flags & VMA_NEEDS_COPY))) {
            rwlock_read_unlock(&v->lock);
            errno_t status = prepare_amap(v, page);
            if (status)
                return status;
            continue;
        }

        const size_t index = page - m->start_page + m->offset_page;
        const size_t slot = page - m->start_page + m->amap_offset;
        struct amap* amap = shared ? nullptr : m->amap;
        errno_t status;

        if (amap) {
            mutex_lock(&amap->lock);
            struct vm_anon* anon = radix_lookup(&amap->anons, slot);
            if (anon)
                status = fault_anon(v, m, anon, page, slot, write, out);
            else if (write || !m->object)
                status = fault_promote(v, m, page, slot, index, out);
            else
                status = fault_object(v, m, page, index, false, out);
            mutex_unlock(&amap->lock);
        } else {
            status = fault_object(v, m, page, index, write, out);
        }

        rwlock_read_unlock(&v->lock);
        if (status != EAGAIN)
            return status;
    }
}

errno_t vma_page_fault(struct vma* v, const struct vma_fault_info* info) {
    const enum fault_kind kind = info->caused_by_write ? FAULT_WRITE : info->caused_by_fetch ? FAULT_EXEC : FAULT_READ;
    phys_t phys;
    return fault(v, info->addr / page_size(), kind, false, &phys);
}

static errno_t write_range(struct vma* v, uintptr_t addr, const void* buf, size_t len) {
    const size_t pgsz = page_size();
    size_t progress = 0;
    while (progress < len) {
        const uintptr_t cur = addr + progress;
        const size_t misalign = cur % pgsz;
        const size_t chunk = MIN(pgsz - misalign, len - progress);

        phys_t phys;
        errno_t status = fault(v, cur / pgsz, FAULT_WRITE, true, &phys);
        if (status)
            return status;

        uint8_t* dst = (uint8_t*)HHDM_PTR(phys) + misalign;
        if (buf)
            memcpy(dst, (const uint8_t*)buf + progress, chunk);
        else
            memset(dst, 0, chunk);
        progress += chunk;
    }
    return 0;
}

errno_t vma_write(struct vma* v, uintptr_t addr, const void* buf, size_t len) {
    return write_range(v, addr, buf, len);
}

errno_t vma_zero(struct vma* v, uintptr_t addr, size_t len) {
    return write_range(v, addr, nullptr, len);
}

errno_t vma_sync(struct vma* v, uintptr_t addr, size_t len) {
    size_t start, end;
    errno_t status = page_range(addr, len, &start, &end);
    if (status)
        return status;

    errno_t result = 0;
    rwlock_read_lock(&v->lock);
    if (!range_mapped(v, start, end)) {
        rwlock_read_unlock(&v->lock);
        return ENOMEM;
    }
    for (struct vma_mapping* m = tailq_first(&v->mappings); m; m = tailq_next(m, link)) {
        if (!m->object || !(m->flags & VMA_SHARED) || m->end_page <= start || m->start_page >= end)
            continue;
        const size_t first = MAX(start, m->start_page) - m->start_page + m->offset_page;
        const size_t last = MIN(end, m->end_page) - m->start_page + m->offset_page;
        status = vmo_sync(m->object, first, last);
        if (status && !result)
            result = status;
    }
    rwlock_read_unlock(&v->lock);
    return result;
}

errno_t vma_map(
    struct vma* vma,
    struct vmo* vmo,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum vma_flags flags,
    uintptr_t vmo_offset
) {
    if (!vma || (!vmo && (flags & VMA_SHARED)))
        return EINVAL;

    uintptr_t end;
    if (len == 0 || ckd_add(&end, addr, len))
        return ENOMEM;

    const size_t pgsz = page_size();
    if (addr % pgsz != vmo_offset % pgsz)
        return EINVAL;

    struct vma_mapping* mapping = mapping_alloc();
    struct vma_mapping* spare = mapping_alloc();
    if (!mapping || !spare) {
        mem_free(mapping, sizeof(*mapping));
        mem_free(spare, sizeof(*spare));
        return ENOMEM;
    }

    *mapping = (struct vma_mapping){
        .start_page = addr / pgsz,
        .end_page = DIV_CEIL(end, pgsz),
        .offset_page = vmo_offset / pgsz,
        .object = vmo,
        .vma = vma,
        .prot = prot,
        .flags = flags & VMA_SHARED,
    };
    if (vmo)
        vmo_ref(vmo);

    mapping_list_t dead;
    tailq_init(&dead);

    rwlock_write_lock(&vma->lock);
    carve(vma, mapping->start_page, mapping->end_page, &spare, &dead);
    pmap_shootdown(&vma->pmap);
    object_link(mapping);
    tailq_insert_tail(&vma->mappings, mapping, link);
    rwlock_write_unlock(&vma->lock);

    free_mappings(&dead);
    mem_free(spare, sizeof(*spare));
    return 0;
}

static bool protect_anons(struct vma* v, struct vma_mapping* m) {
    const size_t pgsz = page_size();
    const size_t num_slots = m->end_page - m->start_page;
    const enum pte_flags flags = prot_to_pte(v, m->prot & ~PROT_WRITE);
    struct vm_anon* batch[32];
    uint64_t slots[32];
    uint64_t next = m->amap_offset;
    bool changed = false;

    mutex_lock(&m->amap->lock);
    while (true) {
        const size_t found = radix_gang_lookup(&m->amap->anons, next, countof(batch), (void**)batch, slots);
        if (found == 0)
            break;
        for (size_t i = 0; i < found; i++) {
            if (slots[i] - m->amap_offset >= num_slots)
                goto done;
            const uintptr_t va = (m->start_page + (slots[i] - m->amap_offset)) * pgsz;
            if (pmap_protect(&v->pmap, va, flags) == 0)
                changed = true;
        }
        next = slots[found - 1] + 1;
    }
done:
    mutex_unlock(&m->amap->lock);
    return changed;
}

errno_t vma_fork(struct vma* src, struct vma** out) {
    struct vma* dst;
    errno_t status = vma_new(&dst);
    if (status)
        return status;

    bool changed = false;
    rwlock_write_lock(&src->lock);
    for (struct vma_mapping* m = tailq_first(&src->mappings); m; m = tailq_next(m, link)) {
        struct vma_mapping* copy = mapping_alloc();
        if (!copy) {
            if (changed)
                pmap_shootdown(&src->pmap);
            rwlock_write_unlock(&src->lock);
            vma_unref(dst);
            return ENOMEM;
        }

        if (!(m->flags & VMA_SHARED) && m->amap) {
            m->flags |= VMA_NEEDS_COPY;
            if ((m->prot & PROT_WRITE) && protect_anons(src, m))
                changed = true;
        }

        mapping_clone(copy, m, dst, m->start_page, m->end_page);
        object_link(copy);
        tailq_insert_tail(&dst->mappings, copy, link);
    }
    if (changed)
        pmap_shootdown(&src->pmap);
    rwlock_write_unlock(&src->lock);

    *out = dst;
    return 0;
}
