// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/iovec.h>
#include <lib/types.h>
#include <stddef.h>
#include <stdint.h>

typedef uint64_t dma_addr_t;

constexpr dma_addr_t DMA_ADDR_MAX = UINT64_MAX;

struct bio;

struct dma_tag;

struct dma_ops {
    errno_t (*map)(struct dma_tag* tag, phys_t phys, size_t len, dma_addr_t* out);
    void (*unmap)(struct dma_tag* tag, dma_addr_t addr, size_t len);
};

struct dma_tag {
    const struct dma_ops* ops;
    void* priv;
    dma_addr_t max_addr;
};

extern struct dma_tag dma_default_tag;

void dma_tag_subregion(struct dma_tag* parent, dma_addr_t max_addr, struct dma_tag* out);

struct dma_seg {
    dma_addr_t addr;
    size_t len;
};

struct dma_buffer {
    struct dma_tag* tag;
    phys_t phys;
    dma_addr_t addr;
    void* virt;
    size_t len;
};

errno_t dma_buffer_alloc(struct dma_tag* tag, size_t len, struct dma_buffer* out);
void dma_buffer_free(struct dma_buffer* buf);

struct dma_map {
    struct dma_tag* tag;
    size_t size;
    size_t max_segs;
    size_t max_seg_size;
    size_t boundary;
    size_t mapped_len;
    size_t num_segs;
    struct dma_seg segs[];
};

enum dma_sync {
    DMA_SYNC_PREREAD = 1 << 0,
    DMA_SYNC_POSTREAD = 1 << 1,
    DMA_SYNC_PREWRITE = 1 << 2,
    DMA_SYNC_POSTWRITE = 1 << 3,
};

errno_t dma_map_create(
    struct dma_tag* tag,
    size_t size,
    size_t max_segs,
    size_t max_seg_size,
    size_t boundary,
    struct dma_map** out
);
void dma_map_destroy(struct dma_map* map);

errno_t dma_map_load(struct dma_map* map, void* buf, size_t len);
errno_t dma_map_load_bio(struct dma_map* map, struct bio* bio);
errno_t dma_map_load_iter(struct dma_map* map, const iovec_iter_t* iter);
void dma_map_unload(struct dma_map* map);

void dma_map_sync(struct dma_map* map, size_t offset, size_t len, enum dma_sync ops);
void dma_buffer_sync(struct dma_buffer* buf, size_t offset, size_t len, enum dma_sync ops);
