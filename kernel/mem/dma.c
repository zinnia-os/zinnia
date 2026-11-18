// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <lib/atomic.h>
#include <lib/percpu.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/dma.h>
#include <mem/layout.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>

static errno_t dma_noop_map(struct dma_tag*, phys_t phys, size_t, dma_addr_t* out) {
    *out = phys;
    return 0;
}

static void dma_noop_unmap(struct dma_tag*, dma_addr_t, size_t) {
    // Does nothing.
}

static const struct dma_ops dma_noop_ops = {
    .map = dma_noop_map,
    .unmap = dma_noop_unmap,
};

struct dma_tag dma_default_tag = {
    .ops = &dma_noop_ops,
    .max_addr = DMA_ADDR_MAX,
};

void dma_tag_subregion(struct dma_tag* parent, dma_addr_t max_addr, struct dma_tag* out) {
    *out = *parent;
    out->max_addr = MIN(parent->max_addr, max_addr);
}

static errno_t tag_alloc_flags(struct dma_tag* tag, enum alloc_flags* out) {
    if (tag->max_addr == DMA_ADDR_MAX)
        *out = 0;
    else if (tag->max_addr >= UINT32_MAX)
        *out = ALLOC_MEM32;
    else if (tag->max_addr >= ((dma_addr_t)1 << 20) - 1)
        *out = ALLOC_MEM20;
    else
        return EINVAL;
    return 0;
}

errno_t dma_buffer_alloc(struct dma_tag* tag, size_t len, struct dma_buffer* out) {
    if (len == 0)
        return EINVAL;

    enum alloc_flags flags;
    errno_t status = tag_alloc_flags(tag, &flags);
    if (status)
        return status;

    const size_t num_pages = DIV_CEIL(len, page_size());
    phys_t phys;
    status = mem_phys_alloc(num_pages, flags, &phys);
    if (status)
        return status;

    const size_t size = num_pages * page_size();
    dma_addr_t addr;
    status = tag->ops->map(tag, phys, size, &addr);
    if (status) {
        mem_phys_free(phys, num_pages);
        return status;
    }
    if (addr + size - 1 > tag->max_addr) {
        tag->ops->unmap(tag, addr, size);
        mem_phys_free(phys, num_pages);
        return ENOMEM;
    }

    *out = (struct dma_buffer){
        .tag = tag,
        .phys = phys,
        .addr = addr,
        .virt = HHDM_PTR(phys),
        .len = len,
    };
    return 0;
}

void dma_buffer_free(struct dma_buffer* buf) {
    if (buf->virt == nullptr)
        return;

    const size_t num_pages = DIV_CEIL(buf->len, page_size());
    buf->tag->ops->unmap(buf->tag, buf->addr, num_pages * page_size());
    mem_phys_free(buf->phys, num_pages);
    *buf = (struct dma_buffer){0};
}

errno_t dma_map_create(
    struct dma_tag* tag,
    size_t size,
    size_t max_segs,
    size_t max_seg_size,
    size_t boundary,
    struct dma_map** out
) {
    if (max_segs == 0 || max_seg_size == 0)
        return EINVAL;
    if (boundary & (boundary - 1))
        return EINVAL;

    struct dma_map* map = mem_alloc(sizeof(*map) + max_segs * sizeof(struct dma_seg), 0);
    if (map == nullptr)
        return ENOMEM;

    map->tag = tag;
    map->size = size;
    map->max_segs = max_segs;
    map->max_seg_size = max_seg_size;
    map->boundary = boundary;
    map->mapped_len = 0;
    map->num_segs = 0;
    *out = map;
    return 0;
}

void dma_map_destroy(struct dma_map* map) {
    mem_free(map, sizeof(*map) + map->max_segs * sizeof(struct dma_seg));
}

static errno_t map_add_range(struct dma_map* map, phys_t phys, size_t len) {
    if (len == 0)
        return 0;

    dma_addr_t addr;
    errno_t status = map->tag->ops->map(map->tag, phys, len, &addr);
    if (status)
        return status;

    if (addr + len - 1 > map->tag->max_addr) {
        map->tag->ops->unmap(map->tag, addr, len);
        return EINVAL;
    }

    while (len > 0) {
        size_t chunk = MIN(len, map->max_seg_size);
        if (map->boundary) {
            const dma_addr_t limit = ROUND_DOWN(addr, map->boundary) + map->boundary;
            chunk = MIN(chunk, limit - addr);
        }

        struct dma_seg* last = map->num_segs ? &map->segs[map->num_segs - 1] : nullptr;
        if (last && last->addr + last->len == addr && last->len + chunk <= map->max_seg_size &&
            (!map->boundary || ROUND_DOWN(last->addr, map->boundary) == ROUND_DOWN(addr + chunk - 1, map->boundary))) {
            last->len += chunk;
        } else {
            if (map->num_segs == map->max_segs) {
                map->tag->ops->unmap(map->tag, addr, len);
                return EFBIG;
            }
            map->segs[map->num_segs++] = (struct dma_seg){.addr = addr, .len = chunk};
        }

        map->mapped_len += chunk;
        addr += chunk;
        len -= chunk;
    }
    return 0;
}

static errno_t map_add_virt(struct dma_map* map, struct pmap* pmap, uintptr_t vaddr, size_t len) {
    const size_t pgsz = page_size();
    while (len > 0) {
        const size_t offset = vaddr % pgsz;
        const size_t chunk = MIN(len, pgsz - offset);

        phys_t phys;
        if (pmap_extract(pmap, vaddr - offset, &phys))
            return EFAULT;

        errno_t status = map_add_range(map, phys + offset, chunk);
        if (status)
            return status;

        vaddr += chunk;
        len -= chunk;
    }
    return 0;
}

static errno_t map_load_finish(struct dma_map* map, errno_t status) {
    if (status)
        dma_map_unload(map);
    return status;
}

errno_t dma_map_load(struct dma_map* map, void* buf, size_t len) {
    if (map->num_segs || len > map->size)
        return EINVAL;

    return map_load_finish(map, map_add_virt(map, &kernel_vma.pmap, (uintptr_t)buf, len));
}

errno_t dma_map_load_bio(struct dma_map* map, struct bio* bio) {
    if (map->num_segs)
        return EINVAL;

    size_t total = 0;
    for (size_t i = 0; i < bio->num_segs; i++)
        total += bio->segs[i].len;
    if (total > map->size)
        return EINVAL;

    errno_t status = 0;
    for (size_t i = 0; i < bio->num_segs && !status; i++)
        status = map_add_range(map, bio->segs[i].addr, bio->segs[i].len);

    return map_load_finish(map, status);
}

errno_t dma_map_load_iter(struct dma_map* map, const iovec_iter_t* iter) {
    if (map->num_segs || iter->total_len - iter->total_offset > map->size)
        return EINVAL;

    struct pmap* pmap = &kernel_vma.pmap;
    if (iter->user)
        pmap = &percpu_get()->sched.current->space->pmap;

    errno_t status = 0;
    size_t offset = iter->current_offset;
    for (size_t i = iter->current_idx; i < iter->iovecs_len && !status; i++) {
        const uintptr_t iov_base =
            iter->user ? (uintptr_t)iter->iovecs[i].iov_base : (uintptr_t)iter->kvecs[i].iov_base;
        const size_t iov_len = iter->user ? iter->iovecs[i].iov_len : iter->kvecs[i].iov_len;
        if (offset >= iov_len) {
            offset = 0;
            continue;
        }

        const uintptr_t base = iov_base + offset;
        const size_t len = iov_len - offset;
        offset = 0;

        if (iter->user && (!mem_addr_is_user(base) || !mem_addr_is_user(base + len - 1))) {
            status = EFAULT;
            break;
        }

        status = map_add_virt(map, pmap, base, len);
    }

    return map_load_finish(map, status);
}

void dma_map_unload(struct dma_map* map) {
    for (size_t i = 0; i < map->num_segs; i++)
        map->tag->ops->unmap(map->tag, map->segs[i].addr, map->segs[i].len);
    map->num_segs = 0;
    map->mapped_len = 0;
}

static void dma_sync(enum dma_sync ops) {
    if (ops & (DMA_SYNC_PREREAD | DMA_SYNC_PREWRITE))
        atomic_fence(ATOMIC_RELEASE);
    if (ops & (DMA_SYNC_POSTREAD | DMA_SYNC_POSTWRITE))
        atomic_fence(ATOMIC_ACQUIRE);
}

void dma_map_sync(struct dma_map* map, size_t offset, size_t len, enum dma_sync ops) {
    dma_sync(ops);
}

void dma_buffer_sync(struct dma_buffer* buf, size_t offset, size_t len, enum dma_sync ops) {
    dma_sync(ops);
}
