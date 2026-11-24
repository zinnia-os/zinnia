// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/errno.h>
#include <lib/iovec.h>
#include <lib/mutex.h>
#include <lib/radix.h>
#include <lib/refcount.h>
#include <lib/tailq.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <stddef.h>
#include <stdint.h>

struct vmo;
struct vma_mapping;

struct vmo_pager_ops {
    errno_t (*get_pages)(struct vmo* vmo, uint64_t index, struct page** pages, size_t count, size_t* out_valid);
    errno_t (*put_pages)(struct vmo* vmo, uint64_t index, struct page** pages, size_t count);
    errno_t (*lookup)(struct vmo* vmo, uint64_t index, phys_t* out);
    size_t max_cluster;
    void (*destroy)(struct vmo* vmo);
};

struct vmo {
    struct refcount rc;
    const struct vmo_pager_ops* pager;
    mutex_t lock;
    struct radix_tree pages;
    tailq_head(struct vma_mapping) mappings;
    uint64_t ra_next;
    size_t ra_window;
    enum cache_mode cache;
};

enum vmo_access {
    VMO_READ,
    VMO_WRITE,
    VMO_OVERWRITE,
};

void vmo_init(struct vmo* vmo, const struct vmo_pager_ops* pager);
void vmo_ref(struct vmo* obj);
void vmo_unref(struct vmo* obj);

errno_t vmo_new_anon(struct vmo** out);
errno_t vmo_get_page(struct vmo* vmo, uint64_t index, enum vmo_access access, struct page** out);
void vmo_put_page(struct page* page);

errno_t vmo_read(struct vmo* obj, uint64_t offset, void* buf, size_t len, size_t* actual_read);
errno_t vmo_write(struct vmo* obj, uint64_t offset, const void* buf, size_t len, size_t* actual_written);
errno_t vmo_read_iter(struct vmo* obj, uint64_t offset, iovec_iter_t* iter, size_t len, size_t* actual_read);
errno_t vmo_write_iter(struct vmo* obj, uint64_t offset, iovec_iter_t* iter, size_t len, size_t* actual_written);
errno_t vmo_copy(
    struct vmo* target,
    uint64_t target_offset,
    struct vmo* src,
    uint64_t src_offset,
    size_t len,
    size_t* actual_copied
);

errno_t vmo_sync(struct vmo* vmo, uint64_t first, uint64_t last);
void vmo_truncate(struct vmo* vmo, uint64_t length);
