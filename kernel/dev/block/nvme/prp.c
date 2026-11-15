// SPDX-License-Identifier: BSD-3-Clause

#include <arch/mem.h>
#include <dev/block/nvme/nvme.h>
#include <lib/utils.h>

static size_t count_pages(struct dma_map* map, size_t page_size) {
    size_t count = 0;
    for (size_t i = 0; i < map->num_segs; i++) {
        const struct dma_seg* seg = &map->segs[i];
        if (seg->len == 0)
            continue;
        const dma_addr_t end = seg->addr + seg->len;
        for (dma_addr_t page = ROUND_DOWN(seg->addr, page_size); page < end; page += page_size)
            count++;
    }
    return count;
}

static void fill_pages(struct dma_map* map, size_t page_size, uint64_t* out) {
    bool first_page = true;
    size_t idx = 0;
    for (size_t i = 0; i < map->num_segs; i++) {
        const struct dma_seg* seg = &map->segs[i];
        if (seg->len == 0)
            continue;
        const dma_addr_t end = seg->addr + seg->len;
        for (dma_addr_t page = ROUND_DOWN(seg->addr, page_size); page < end; page += page_size) {
            if (first_page)
                first_page = false;
            else
                out[idx++] = page;
        }
    }
}

errno_t nvme_build_prps(
    struct dma_tag* tag,
    struct bio* bio,
    struct nvme_inflight* slot,
    uint64_t* prp1,
    uint64_t* prp2
) {
    const size_t page_size = arch_mem_page_size();
    slot->map = nullptr;
    slot->prps = (struct dma_buffer){};

    if (bio->num_segs == 0)
        return EIO;

    errno_t status = dma_map_create(tag, SIZE_MAX, bio->num_segs, SIZE_MAX, 0, &slot->map);
    if (status)
        return status;

    status = dma_map_load_bio(slot->map, bio);
    if (status)
        goto fail;
    if (slot->map->num_segs == 0) {
        status = EIO;
        goto fail;
    }

    dma_map_sync(slot->map, 0, slot->map->mapped_len, bio->op == BIO_WRITE ? DMA_SYNC_PREWRITE : DMA_SYNC_PREREAD);

    *prp1 = slot->map->segs[0].addr;

    // Enumerate the base of every page the transfer touches in order.
    const size_t pages = count_pages(slot->map, page_size);
    const size_t rest_pages = pages ? pages - 1 : 0;

    if (rest_pages == 0) {
        *prp2 = 0;
        return 0;
    }

    if (rest_pages == 1) {
        uint64_t second;
        fill_pages(slot->map, page_size, &second);
        *prp2 = second;
        return 0;
    }

    // We don't want to chain across pages.
    const size_t entries_per_page = page_size / sizeof(uint64_t);
    if (rest_pages > entries_per_page) {
        status = ENOMEM;
        goto fail;
    }

    status = dma_buffer_alloc(tag, page_size, &slot->prps);
    if (status)
        goto fail;

    fill_pages(slot->map, page_size, slot->prps.virt);
    dma_buffer_sync(&slot->prps, 0, rest_pages * sizeof(uint64_t), DMA_SYNC_PREWRITE);

    *prp2 = slot->prps.addr;
    return 0;

fail:
    nvme_release_prps(slot);
    return status;
}

void nvme_release_prps(struct nvme_inflight* slot) {
    if (slot->map) {
        dma_map_sync(
            slot->map,
            0,
            slot->map->mapped_len,
            slot->bio && slot->bio->op == BIO_WRITE ? DMA_SYNC_POSTWRITE : DMA_SYNC_POSTREAD
        );
        dma_map_unload(slot->map);
        dma_map_destroy(slot->map);
        slot->map = nullptr;
    }
    dma_buffer_free(&slot->prps);
}
